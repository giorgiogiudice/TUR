#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""P@x through the accessibility tree, end to end, under a real KWin.

Run by tests/pax/a11y/run.sh inside the image of tests/pax/a11y/Containerfile
(the `accessibility` job of .github/workflows/pax.yml). It starts:

  - a private session bus (dbus-daemon);
  - KWin, headless (its virtual backend, Mesa's software renderer);
  - the accessibility bus and its registry: at-spi2-core's launcher, by
    D-Bus activation, as a desktop session starts it (or, if that fails,
    the same daemons by hand: the report says which);
  - tur-pax-overlay, P@x's indicator and pointer;
  - the applications, one group at a time: tur-settings (Qt Quick), a GTK 3
    test window (mail_app.py), gnome-text-editor (GTK 4), Firefox with a
    test page, Wine's own notepad.

For each application it measures what the accessibility tree exposes
(`tur-pax-a11y survey`), then runs P@x on it: `tur-pax run --interface`,
the real task loop, the real policy engine and the real bridge, against a
scripted provider (tests/pax/fake_provider.py). No model is called. While a
task runs, the bridge's log says when P@x's pointer has arrived at an
element and is about to act; a screenshot is taken from KWin then.

It writes to OUT: report.md, results.json, coverage.json, each task's
transcript, audit trail and bridge log, the screenshots, and every
process's log. It exits 1 if a hard check failed. Firefox and Wine are
measured and reported; what they do not expose does not fail the run.

    harness.py --out DIR --build DIR --kwin PATH --tur-pax PATH --bridge PATH
"""

import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import threading
import time

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from fake_provider import ScriptedProvider  # noqa: E402

KEY = "sk-test-not-a-real-key-a11y"
GOLD = (0xE8, 0xB4, 0x4C)
MAIL_TITLE = "TUR test mail"
PAGE_TITLE = "TUR accessibility test page"


class Checks:
    """Hard and soft checks, in order."""

    def __init__(self):
        self.items = []

    def check(self, scenario, what, ok, detail="", hard=True):
        self.items.append({"scenario": scenario, "check": what, "ok": bool(ok), "hard": hard, "detail": str(detail)[:500]})
        mark = "ok  " if ok else ("FAIL" if hard else "note")
        print(f"  [{mark}] {scenario}: {what}" + (f" -- {str(detail)[:300]}" if detail and not ok else ""), flush=True)

    def failed(self):
        return [item for item in self.items if item["hard"] and not item["ok"]]


def find_program(build, name):
    for path in sorted(pathlib.Path(build).rglob(name)):
        if path.is_file() and os.access(path, os.X_OK):
            return path
    return None


def gdbus(environment, *arguments, timeout=20):
    process = subprocess.run(
        ["gdbus", "call", *arguments], env=environment, capture_output=True, text=True, timeout=timeout
    )
    return process.returncode, process.stdout.strip(), process.stderr.strip()


class Session:
    """The private desktop session: bus, compositor, accessibility, overlay."""

    def __init__(self, out, kwin):
        self.out = out
        self.kwin = kwin
        self.runtime = pathlib.Path(tempfile.mkdtemp(prefix="tur-a11y-"))
        self.runtime.chmod(0o700)
        self.home = self.runtime / "home"
        self.home.mkdir()
        (self.runtime / "config").mkdir()
        self.processes = {}
        self.logs = out / "logs"
        self.logs.mkdir(parents=True, exist_ok=True)
        self.a11y = {"how": "not started"}

    def environment(self, **extra):
        environment = {
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "HOME": str(self.home),
            "LANG": "C.UTF-8",
            "LC_ALL": "C.UTF-8",
            "XDG_RUNTIME_DIR": str(self.runtime),
            "XDG_CONFIG_HOME": str(self.runtime / "config"),
            "XDG_DATA_HOME": str(self.home / ".local/share"),
            "XDG_CACHE_HOME": str(self.home / ".cache"),
            "DBUS_SESSION_BUS_ADDRESS": f"unix:path={self.runtime}/bus",
            "WAYLAND_DISPLAY": "wayland-tur",
            "XDG_SESSION_TYPE": "wayland",
            "XCURSOR_SIZE": "24",
        }
        if "AT_SPI_BUS_ADDRESS" in self.a11y:
            environment["AT_SPI_BUS_ADDRESS"] = self.a11y["AT_SPI_BUS_ADDRESS"]
        environment.update(extra)
        return environment

    def start(self, name, command, **extra):
        log = open(self.logs / f"{name}.log", "w", encoding="utf-8")
        process = subprocess.Popen(
            [str(part) for part in command],
            env=self.environment(**extra),
            stdout=log,
            stderr=subprocess.STDOUT,
            stdin=subprocess.DEVNULL,
        )
        self.processes[name] = process
        return process

    def stop(self, name):
        process = self.processes.pop(name, None)
        if process is None:
            return
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()

    def wait_for(self, predicate, timeout, step=0.25):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if predicate():
                return True
            time.sleep(step)
        return predicate()

    def name_owned(self, name):
        code, out, _ = gdbus(
            self.environment(),
            "--session",
            "--dest",
            "org.freedesktop.DBus",
            "--object-path",
            "/org/freedesktop/DBus",
            "--method",
            "org.freedesktop.DBus.NameHasOwner",
            name,
        )
        return code == 0 and "true" in out

    def up(self):
        bus = self.runtime / "bus"
        self.start("dbus", ["dbus-daemon", "--session", "--nofork", "--nopidfile", f"--address=unix:path={bus}"])
        if not self.wait_for(bus.exists, 10):
            raise RuntimeError("the session bus did not start")

        self.start(
            "kwin",
            [
                self.kwin,
                "--virtual",
                "--width",
                "1280",
                "--height",
                "800",
                "--socket",
                "wayland-tur",
                "--no-lockscreen",
                "--no-global-shortcuts",
                "--no-kactivities",
            ],
        )
        if not self.wait_for((self.runtime / "wayland-tur").exists, 40):
            raise RuntimeError("kwin_wayland did not open its socket")
        time.sleep(2)

        self.accessibility_up()

    def accessibility_up(self):
        """The accessibility bus, as a session gets it: by activation of
        at-spi2-core's launcher. If that does not work in the container, the
        same two daemons are started by hand, and the report says so."""
        environment = self.environment()
        code, out, err = gdbus(
            environment,
            "--session",
            "--dest",
            "org.a11y.Bus",
            "--object-path",
            "/org/a11y/bus",
            "--method",
            "org.a11y.Bus.GetAddress",
            timeout=30,
        )
        address = ""
        if code == 0 and "'" in out:
            address = out.split("'")[1]
        registry = False
        if address:
            registry = self.registry_answers(address)
        if address and registry:
            self.a11y = {"how": "at-spi-bus-launcher, activated on the session bus", "address": address}
        else:
            launcher_error = (err or out)[:300]
            a11y_bus = self.runtime / "a11y-bus"
            config = "/usr/share/defaults/at-spi2/accessibility.conf"
            self.start(
                "a11y-bus",
                ["dbus-daemon", f"--config-file={config}", "--nofork", "--nopidfile", f"--address=unix:path={a11y_bus}"],
            )
            if not self.wait_for(a11y_bus.exists, 10):
                raise RuntimeError(f"no accessibility bus: the launcher said {launcher_error}")
            address = f"unix:path={a11y_bus}"
            self.a11y = {
                "how": "dbus-daemon with at-spi2-core's configuration and at-spi2-registryd, started by hand "
                f"(the launcher did not answer: {launcher_error})",
                "address": address,
                "AT_SPI_BUS_ADDRESS": address,
            }
            registryd = next(
                (path for path in ["/usr/libexec/at-spi2-registryd", "/usr/lib/at-spi2-core/at-spi2-registryd"] if os.path.exists(path)),
                "/usr/libexec/at-spi2-registryd",
            )
            self.start("registryd", [registryd])
            if not self.wait_for(lambda: self.registry_answers(address), 20, step=0.5):
                raise RuntimeError("the accessibility registry does not answer")
        # Accessibility switched on for the session, as a screen reader or
        # TUR's settings would: toolkits that ask expose their trees.
        code, out, err = gdbus(
            environment,
            "--session",
            "--dest",
            "org.a11y.Bus",
            "--object-path",
            "/org/a11y/bus",
            "--method",
            "org.freedesktop.DBus.Properties.Set",
            "org.a11y.Status",
            "IsEnabled",
            "<true>",
        )
        self.a11y["is_enabled_set"] = code == 0
        self.a11y["is_enabled_detail"] = (err or out)[:200]
        print(f"accessibility bus: {self.a11y}", flush=True)

    def registry_answers(self, address):
        code, out, _ = gdbus(
            self.environment(),
            "--address",
            address,
            "--dest",
            "org.a11y.atspi.Registry",
            "--object-path",
            "/org/a11y/atspi/accessible/root",
            "--method",
            "org.a11y.atspi.Accessible.GetRole",
            timeout=15,
        )
        return code == 0

    def down(self):
        for name in list(self.processes)[::-1]:
            self.stop(name)
        shutil.rmtree(self.runtime, ignore_errors=True)


class Shooter:
    """Screenshots from KWin (org.kde.KWin.ScreenShot2, CaptureActiveScreen),
    with the test's own D-Bus client (Gio), independent of tur-pax's."""

    def __init__(self, session, directory):
        self.session = session
        self.directory = directory
        directory.mkdir(parents=True, exist_ok=True)
        self.bus = None
        self.taken = []

    def connect(self):
        if self.bus is None:
            from gi.repository import Gio  # the image has python3-gobject

            address = self.session.environment()["DBUS_SESSION_BUS_ADDRESS"]
            self.bus = Gio.DBusConnection.new_for_address_sync(
                address,
                Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
                None,
                None,
            )
        return self.bus

    def take(self, name):
        from gi.repository import Gio, GLib
        from PIL import Image

        bus = self.connect()
        read_end, write_end = os.pipe()
        fds = Gio.UnixFDList.new()
        index = fds.append(write_end)
        os.close(write_end)
        options = {
            "native-resolution": GLib.Variant("b", True),
            "include-cursor": GLib.Variant("b", False),
        }
        try:
            reply, _ = bus.call_with_unix_fd_list_sync(
                "org.kde.KWin",
                "/org/kde/KWin/ScreenShot2",
                "org.kde.KWin.ScreenShot2",
                "CaptureActiveScreen",
                GLib.Variant("(a{sv}h)", (options, index)),
                GLib.VariantType("(a{sv})"),
                Gio.DBusCallFlags.NONE,
                30000,
                fds,
                None,
            )
        except Exception as error:  # noqa: BLE001 - reported, not raised
            os.close(read_end)
            del fds
            return {"name": name, "error": str(error)}
        del fds
        results = reply.unpack()[0]
        width, height, stride = results.get("width", 0), results.get("height", 0), results.get("stride", 0)
        image_format = results.get("format", 0)
        wanted = stride * height
        data = bytearray()
        while len(data) < wanted:
            chunk = os.read(read_end, min(1 << 20, wanted - len(data)))
            if not chunk:
                break
            data.extend(chunk)
        os.close(read_end)
        if width <= 0 or height <= 0 or len(data) != wanted:
            return {"name": name, "error": f"incomplete: {results} {len(data)} bytes"}
        # QImage formats: 4 RGB32, 5 ARGB32, 6 ARGB32 premultiplied are
        # BGRA in memory on a little-endian machine; 16 RGBX8888, 17
        # RGBA8888, 18 RGBA8888 premultiplied are RGBA.
        raw = "RGBA" if image_format in (16, 17, 18) else "BGRA"
        image = Image.frombuffer("RGBA", (width, height), bytes(data), "raw", raw, stride, 1).convert("RGB")
        path = self.directory / f"{name}.png"
        image.save(path)
        gold = gold_pixels(image)
        shot = {"name": name, "path": str(path.relative_to(self.directory.parent)), "width": width, "height": height, **gold}
        self.taken.append(shot)
        return shot


def gold_pixels(image):
    """How many pixels are P@x's gold (#E8B44C, within 24 on each channel, as
    desktop/tests/compositor does it), and where they are."""
    pixels = image.load()
    width, height = image.size
    count = 0
    left, top, right, bottom = width, height, -1, -1
    for y in range(0, height, 2):
        for x in range(0, width, 2):
            r, g, b = pixels[x, y][:3]
            if abs(r - GOLD[0]) <= 24 and abs(g - GOLD[1]) <= 24 and abs(b - GOLD[2]) <= 24:
                count += 1
                left, top, right, bottom = min(left, x), min(top, y), max(right, x), max(bottom, y)
    box = [left, top, right, bottom] if count else []
    return {"gold_pixels_sampled": count, "gold_box": box}


class Harness:
    def __init__(self, arguments):
        self.arguments = arguments
        self.out = pathlib.Path(arguments.out).resolve()
        self.out.mkdir(parents=True, exist_ok=True)
        self.tasks = self.out / "tasks"
        self.tasks.mkdir(exist_ok=True)
        self.session = Session(self.out, arguments.kwin)
        self.shooter = Shooter(self.session, self.out / "screenshots")
        self.checks = Checks()
        self.results = {"scenarios": {}, "coverage": {}, "notes": []}
        self.tur_pax = pathlib.Path(arguments.tur_pax).resolve()
        self.bridge = pathlib.Path(arguments.bridge).resolve()
        self.build = pathlib.Path(arguments.build).resolve()
        self.baseline_gold = 0
        self.empty_tools = self.out / "no-tools"
        self.empty_tools.mkdir(exist_ok=True)

    # ------------------------------------------------------------ surveys

    def survey(self):
        process = subprocess.run(
            [str(self.bridge), "survey"], env=self.session.environment(), capture_output=True, text=True, timeout=120
        )
        if process.returncode != 0:
            return {"error": process.stderr.strip()[:500], "applications": []}
        try:
            return json.loads(process.stdout)
        except json.JSONDecodeError as error:
            return {"error": f"not JSON: {error}", "applications": []}

    def application(self, survey, predicate):
        for application in survey.get("applications", []):
            if application.get("excluded"):
                continue
            names = [application.get("application", "")] + list(application.get("also_known_as", []))
            if predicate(names, application):
                return application
        return None

    def wait_for_application(self, label, predicate, timeout):
        found = {}

        def ready():
            survey = self.survey()
            application = self.application(survey, predicate)
            if application and application.get("windows"):
                found["survey"] = survey
                found["application"] = application
                return True
            found["last"] = survey
            return False

        self.session.wait_for(ready, timeout, step=2)
        (self.out / f"survey-{label}.json").write_text(json.dumps(found.get("survey") or found.get("last"), indent=2))
        return found.get("application")

    def pointed(self, shot):
        """Whether a screenshot shows clearly more of P@x's gold than the
        indicator alone: the pointer and the highlight round an element."""
        return shot.get("gold_pixels_sampled", 0) > self.baseline_gold + 100

    # ----------------------------------------------------------- the tasks

    def run_task(self, name, request, turns, on_arrival=None):
        """One task of P@x: tur-pax run --interface against a scripted
        provider. Screenshots are taken when the bridge says P@x's pointer
        has arrived."""
        print(f"\n== task {name}: {request}", flush=True)
        provider = ScriptedProvider(turns).start()
        config = {
            "schema_version": 1,
            "provider": "anthropic",
            "base_url": provider.base_url,
            "model": "scripted-model",
            "max_output_tokens": 1024,
            "timeouts": {"connect_ms": 5000, "response_ms": 60000, "idle_ms": 60000},
            "retry": {"max_attempts": 1, "initial_delay_ms": 10, "max_delay_ms": 100},
        }
        config_path = self.tasks / f"{name}.provider.json"
        config_path.write_text(json.dumps(config, indent=2))
        audit_path = self.tasks / f"{name}.audit.jsonl"
        if audit_path.exists():
            audit_path.unlink()
        command = [
            str(self.tur_pax),
            "run",
            request,
            "--json",
            "--provider-config",
            str(config_path),
            "--interface",
            "--interface-bridge",
            str(self.bridge),
            "--no-commands",
            "--tools",
            str(self.empty_tools),
            "--audit-log",
            str(audit_path),
            "--task",
            name,
            "--max-seconds",
            "240",
            "--max-steps",
            "12",
        ]
        process = subprocess.Popen(
            command,
            env=self.session.environment(TUR_PAX_API_KEY=KEY),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        lines = []
        shots = []

        def follow():
            for line in process.stderr:
                lines.append(line.rstrip("\n"))
                if "TUR-A11Y arrived=" in line:
                    arrival = len([text for text in lines if "TUR-A11Y arrived=" in text])
                    if on_arrival is not None:
                        on_arrival(arrival)
                    shot = self.shooter.take(f"{name}-{arrival}")
                    shot["log"] = line.strip()
                    shots.append(shot)

        follower = threading.Thread(target=follow, daemon=True)
        follower.start()
        # Standard error is the follower's alone: the transcript is read
        # here, and the task is ended if it outlives its bound.
        bound = threading.Timer(400, process.kill)
        bound.start()
        stdout = process.stdout.read()
        process.wait()
        bound.cancel()
        follower.join(timeout=30)
        provider.stop()

        transcript = {}
        try:
            transcript = json.loads(stdout) if stdout.strip() else {}
        except json.JSONDecodeError:
            transcript = {"unparsed": stdout[:2000]}
        audit = []
        if audit_path.exists():
            audit = [json.loads(line) for line in audit_path.read_text().splitlines() if line.strip()]
        (self.tasks / f"{name}.transcript.json").write_text(json.dumps(transcript, indent=2))
        (self.tasks / f"{name}.stderr.txt").write_text("\n".join(lines) + "\n")
        (self.tasks / f"{name}.requests.json").write_text(json.dumps(provider.bodies(), indent=2))
        results = tool_results(provider.bodies())
        record = {
            "request": request,
            "status": process.returncode,
            "outcome": transcript.get("outcome"),
            "error_code": transcript.get("error_code"),
            "proposals": [item for item in transcript.get("items", []) if item.get("type") == "proposal"],
            "decisions": [entry for entry in audit if entry.get("TUR_AI_EVENT") == "decision"],
            "executed": [entry for entry in audit if entry.get("TUR_AI_EVENT") == "executed"],
            "rejected": [entry for entry in audit if entry.get("TUR_AI_EVENT") == "proposal_rejected"],
            "results": results,
            "screenshots": shots,
            "bridge_log": [line for line in lines if "TUR-A11Y" in line],
        }
        self.results["scenarios"][name] = record
        # The trail: every proposal was decided or rejected before anything
        # ran, and every performed action has an outcome after its decision.
        events = [entry.get("TUR_AI_EVENT") for entry in audit]
        self.checks.check(name, "the trail begins and ends with the task", events[:1] == ["task_started"] and events[-1:] == ["task_finished"], events)
        self.checks.check(
            name,
            "every proposal is in the trail as a decision or a rejection",
            len(record["decisions"]) + len(record["rejected"]) == len(record["proposals"]),
            f"{len(record['decisions'])} decisions, {len(record['rejected'])} rejected, {len(record['proposals'])} proposals",
        )
        order_ok = True
        seen_decisions = 0
        for event in events:
            if event == "decision":
                seen_decisions += 1
            if event == "executed" and seen_decisions == 0:
                order_ok = False
        self.checks.check(name, "nothing was performed before a decision", order_ok, events)
        above = [entry for entry in record["decisions"] if int(entry.get("TUR_AI_CLASS", "0")) >= 2]
        self.checks.check(
            name,
            "no decision above class 1 is anything but refused",
            all(entry.get("TUR_AI_DECISION") == "refused" for entry in above),
            above,
        )
        return record

    # ----------------------------------------------------------- phases

    def phase_settings(self):
        program = find_program(self.build, "tur-settings")
        if program is None:
            self.checks.check("settings", "tur-settings was built", False)
            return
        fixtures = pathlib.Path("desktop/apps/tur-settings/fixtures/default").resolve()
        self.session.start(
            "tur-settings",
            [program, "--standalone", "--variant", "dark", "--fixture", fixtures, "--registry", self.out / "no-registries"],
            QT_QPA_PLATFORM="wayland",
            QT_QUICK_BACKEND="software",
            QT_LINUX_ACCESSIBILITY_ALWAYS_ON="1",
        )
        application = self.wait_for_application(
            "qt", lambda names, app: any(name in ("org.tur.Settings", "tur-settings") for name in names), 60
        )
        self.results["coverage"]["qt"] = application or {}
        self.checks.check("settings", "tur-settings is on the accessibility bus with a window", application is not None)
        if application is None:
            self.session.stop("tur-settings")
            return
        record = self.run_task(
            "settings-appearance",
            "Open the Appearance page in Settings and tell me the current theme.",
            [
                {"text": "Let me look at the windows.", "calls": [{"id": "t1", "name": "ui_list_windows", "arguments": {}}]},
                {"calls": [{"id": "t2", "name": "ui_find", "arguments": {"window": "Settings", "name": "Appearance"}}]},
                {"calls": [{"id": "t3", "name": "ui_press", "arguments": {"window": "Settings", "name": "Appearance", "role": "list item"}}]},
                {"calls": [{"id": "t4", "name": "ui_focus", "arguments": {"window": "Settings", "name": "Appearance", "role": "list item"}}]},
                {"calls": [{"id": "t5", "name": "ui_find", "arguments": {"window": "Settings", "name": "desktop is set to"}}]},
                {"text": "I looked at the Appearance page of Settings."},
            ],
        )
        results = record["results"]
        decisions = [entry.get("TUR_AI_DECISION") for entry in record["decisions"]]
        self.checks.check("settings-appearance", "the task completed", record["outcome"] == "completed", record["outcome"])
        self.checks.check("settings-appearance", "all five calls were allowed (classes 0, 0, 1, 1, 0)", decisions == ["allowed"] * 5, decisions)
        listed = results[0].get("output", {}).get("windows", []) if results else []
        self.checks.check("settings-appearance", "the list of windows has Settings", any(window.get("title") == "Settings" for window in listed), listed)
        found = results[1].get("output", {}).get("matches", []) if len(results) > 1 else []
        self.checks.check("settings-appearance", "the Appearance row was found by its name", len(found) >= 1, found)
        press = results[2] if len(results) > 2 else {}
        focus = results[3] if len(results) > 3 else {}
        theme = results[4].get("output", {}).get("matches", []) if len(results) > 4 else []
        self.checks.check("settings-appearance", "giving the Appearance row the keyboard worked", focus.get("status") == "ok", focus)
        opened = press.get("status") == "ok"
        self.results["notes"].append(
            {
                "settings_press": press,
                "settings_page_opened": opened,
                "settings_theme_rows": theme,
            }
        )
        self.checks.check(
            "settings-appearance",
            "pressing the row opened the page, or the bridge said exactly why not",
            opened or "offers no action" in json.dumps(press),
            press,
        )
        self.checks.check(
            "settings-appearance",
            "P@x's pointer was photographed at an element (gold on screen)",
            any(self.pointed(shot) for shot in record["screenshots"]),
            record["screenshots"],
        )
        self.session.stop("tur-settings")

    def phase_mail(self):
        log = self.out / "mail.log"
        log.write_text("")
        self.session.start("mail", ["python3", HERE / "mail_app.py", log], GDK_BACKEND="wayland")
        application = self.wait_for_application(
            "gtk3", lambda names, app: "tur-a11y-mail" in names or "TUR test mail" in names, 60
        )
        self.results["coverage"]["gtk3"] = application or {}
        self.checks.check("mail", "the GTK 3 window is on the accessibility bus", application is not None)
        if application is None:
            self.session.stop("mail")
            return

        def logged():
            return log.read_text().splitlines()

        record = self.run_task(
            "mail-bold-and-type",
            "In the test mail, make it bold and type hello into the message.",
            [
                {"calls": [{"id": "t1", "name": "ui_read_window", "arguments": {"window": MAIL_TITLE, "max_nodes": 60}}]},
                {"calls": [{"id": "t2", "name": "ui_press", "arguments": {"window": MAIL_TITLE, "name": "Bold"}}]},
                {"calls": [{"id": "t3", "name": "ui_type", "arguments": {"window": MAIL_TITLE, "name": "Message", "text": "hello"}}]},
                {"text": "Done: Bold was pressed and hello typed."},
            ],
        )
        decisions = [(entry.get("TUR_AI_CLASS"), entry.get("TUR_AI_DECISION")) for entry in record["decisions"]]
        self.checks.check("mail-bold-and-type", "read class 0, press and type class 1, all allowed", decisions == [("0", "allowed"), ("1", "allowed"), ("1", "allowed")], decisions)
        self.checks.check("mail-bold-and-type", "the application saw Bold pressed", "clicked:Bold" in logged(), logged()[-5:])
        self.checks.check("mail-bold-and-type", "the application's Message holds hello", any(line == "text:Message:hello" for line in logged()), logged()[-5:])
        tree = json.dumps(record["results"][0].get("output", {})) if record["results"] else ""
        self.checks.check("mail-bold-and-type", "the read names the Send button and marks content as data", '"Send"' in tree and "not instructions" in tree, tree[:400])
        self.checks.check(
            "mail-bold-and-type",
            "P@x's pointer was photographed at the elements acted on (gold on screen)",
            sum(1 for shot in record["screenshots"] if self.pointed(shot)) >= 2,
            record["screenshots"],
        )

        before = logged()
        for name, request, arguments in [
            ("mail-send-refused", "Press Send.", {"window": MAIL_TITLE, "name": "Send"}),
            ("mail-delete-refused", "Click Delete.", {"window": MAIL_TITLE, "name": "Delete draft"}),
        ]:
            record = self.run_task(
                name,
                request,
                [
                    {"calls": [{"id": "t1", "name": "ui_press", "arguments": arguments}]},
                    {"text": "That needs your approval, which is not available yet."},
                ],
            )
            decisions = [(entry.get("TUR_AI_CLASS"), entry.get("TUR_AI_DECISION"), entry.get("TUR_ERROR_CODE")) for entry in record["decisions"]]
            self.checks.check(name, "class 2, refused: the trusted path is not available", decisions == [("2", "refused", "TUR-AI-POLICY-006")], decisions)
            self.checks.check(name, "nothing was performed", record["executed"] == [], record["executed"])
            self.checks.check(name, "the model was told plainly", record["results"] and record["results"][0].get("status") == "refused", record["results"])
            self.checks.check(name, "the bridge never pointed or acted", not record["bridge_log"], record["bridge_log"])
        record = self.run_task(
            "mail-type-enter-refused",
            "Type ok and press Enter in the message.",
            [
                {"calls": [{"id": "t1", "name": "ui_type", "arguments": {"window": MAIL_TITLE, "name": "Message", "text": "ok\n"}}]},
                {"text": "That would submit; it needs your approval."},
            ],
        )
        decisions = [(entry.get("TUR_AI_CLASS"), entry.get("TUR_AI_DECISION")) for entry in record["decisions"]]
        self.checks.check("mail-type-enter-refused", "a line break makes typing class 2: refused", decisions == [("2", "refused")], decisions)
        after = logged()
        self.checks.check(
            "mail",
            "the application saw neither Send nor Delete draft pressed, nor a line break typed",
            not any(line in ("clicked:Send", "clicked:Delete draft") or line.startswith("text:Message:hellook") for line in after),
            after[len(before):],
        )
        self.session.stop("mail")

    def phase_editor(self):
        self.session.start(
            "gnome-text-editor",
            ["gnome-text-editor", "--standalone", "--ignore-session", "--new-window"],
            GDK_BACKEND="wayland",
            GSK_RENDERER="cairo",
            GSETTINGS_BACKEND="memory",
        )
        application = self.wait_for_application(
            "gtk4", lambda names, app: any("texteditor" in name.lower() or name == "gnome-text-editor" for name in names), 60
        )
        self.results["coverage"]["gtk4"] = application or {}
        self.checks.check("editor", "gnome-text-editor (GTK 4) is on the accessibility bus", application is not None, hard=False)
        target_window = application["application"] if application else None
        fallback = False
        if application is None:
            # The task is run against the GTK 3 test window instead, and the
            # report says so.
            fallback = True
            log = self.out / "mail-editor.log"
            log.write_text("")
            self.session.start("mail-editor", ["python3", HERE / "mail_app.py", log], GDK_BACKEND="wayland")
            self.wait_for_application("gtk3-again", lambda names, app: "tur-a11y-mail" in names, 60)
            target_window = MAIL_TITLE
        self.results["notes"].append({"editor_fallback_to_gtk3_window": fallback})
        record = self.run_task(
            "editor-hello",
            "Type 'hello' into the open text editor.",
            [
                {"calls": [{"id": "t1", "name": "ui_list_windows", "arguments": {}}]},
                {"calls": [{"id": "t2", "name": "ui_type", "arguments": {"window": target_window, "text": "hello"}}]},
                {"calls": [{"id": "t3", "name": "ui_read_focused", "arguments": {}}]},
                {"text": "I typed hello into the editor."},
            ],
        )
        decisions = [(entry.get("TUR_AI_CLASS"), entry.get("TUR_AI_DECISION")) for entry in record["decisions"]]
        self.checks.check("editor-hello", "list class 0, typing class 1: allowed", decisions[:2] == [("0", "allowed"), ("1", "allowed")], decisions)
        typed = record["results"][1].get("output", {}) if len(record["results"]) > 1 else {}
        self.checks.check("editor-hello", "the text field holds hello, read back from it", typed.get("text_now_contains_typed_text") is True, typed)
        self.checks.check(
            "editor-hello",
            "P@x's pointer was photographed at the text field (gold on screen)",
            any(self.pointed(shot) for shot in record["screenshots"]),
            record["screenshots"],
        )
        self.session.stop("gnome-text-editor")
        self.session.stop("mail-editor")

    def phase_firefox(self):
        profile = self.session.home / "firefox-profile"
        profile.mkdir(parents=True, exist_ok=True)
        (profile / "user.js").write_text(
            "\n".join(
                [
                    'user_pref("browser.shell.checkDefaultBrowser", false);',
                    'user_pref("browser.startup.homepage_override.mstone", "ignore");',
                    'user_pref("browser.aboutwelcome.enabled", false);',
                    'user_pref("datareporting.policy.dataSubmissionEnabled", false);',
                    'user_pref("toolkit.telemetry.reportingpolicy.firstRun", false);',
                    'user_pref("app.update.disabledForTesting", true);',
                    'user_pref("browser.tabs.warnOnClose", false);',
                    'user_pref("accessibility.force_disabled", 0);',
                    "",
                ]
            )
        )
        page = (HERE / "page.html").resolve()
        self.session.start(
            "firefox",
            ["firefox", "--no-remote", "--profile", profile, "--new-instance", f"file://{page}"],
            MOZ_ENABLE_WAYLAND="1",
            GNOME_ACCESSIBILITY="1",
            MOZ_CRASHREPORTER_DISABLE="1",
        )
        application = self.wait_for_application(
            "firefox", lambda names, app: any("firefox" in name.lower() for name in names), 120
        )
        self.results["coverage"]["firefox"] = application or {}
        self.checks.check("firefox", "Firefox is on the accessibility bus with a window", application is not None, hard=False)
        if application is None:
            self.session.stop("firefox")
            return
        time.sleep(3)
        record = self.run_task(
            "firefox-page",
            "On the test page, type hello into the search terms and preview it.",
            [
                {"calls": [{"id": "t1", "name": "ui_find", "arguments": {"window": PAGE_TITLE, "role": "text"}}]},
                {"calls": [{"id": "t2", "name": "ui_type", "arguments": {"window": PAGE_TITLE, "name": "Search terms", "text": "hello"}}]},
                {"calls": [{"id": "t3", "name": "ui_press", "arguments": {"window": PAGE_TITLE, "name": "Preview"}}]},
                {"calls": [{"id": "t4", "name": "ui_press", "arguments": {"window": PAGE_TITLE, "name": "Send"}}]},
                {"calls": [{"id": "t5", "name": "ui_read_window", "arguments": {"window": PAGE_TITLE, "max_nodes": 80}}]},
                {"text": "I typed hello and previewed it; Send needs your approval."},
            ],
        )
        results = record["results"]
        statuses = [result.get("status") for result in results]
        page_text = json.dumps(results[4].get("output", {})) if len(results) > 4 else ""
        decisions = [(entry.get("TUR_AI_CLASS"), entry.get("TUR_AI_DECISION")) for entry in record["decisions"]]
        self.results["notes"].append({"firefox_statuses": statuses, "firefox_decisions": decisions, "firefox_previewed": "Previewed: hello" in page_text})
        self.checks.check("firefox-page", "Send on the page was refused as class 2", ("2", "refused") in decisions, decisions)
        self.checks.check("firefox-page", "typing and pressing Preview reached the page", "Previewed: hello" in page_text, page_text[:400], hard=False)
        self.session.stop("firefox")

    def phase_wine(self):
        environment = {
            "WINEPREFIX": str(self.session.home / "wine"),
            "WINEDEBUG": "-all",
            "WINEDLLOVERRIDES": "mscoree,mshtml=",
        }
        before = self.survey()
        boot = subprocess.run(
            ["wine", "wineboot", "--init"],
            env=self.session.environment(**environment),
            capture_output=True,
            text=True,
            timeout=300,
        )
        (self.session.logs / "wineboot.log").write_text(boot.stdout + boot.stderr)
        self.session.start("wine-notepad", ["wine", "notepad"], **environment)
        time.sleep(30)
        after = self.survey()
        (self.out / "survey-wine.json").write_text(json.dumps(after, indent=2))
        shot = self.shooter.take("wine-notepad")
        known = {app.get("application") for app in before.get("applications", [])}
        new = [app for app in after.get("applications", []) if app.get("application") not in known]
        wine_like = [
            app
            for app in after.get("applications", [])
            if any(word in " ".join([app.get("application", "")] + app.get("also_known_as", [])).lower() for word in ("wine", "notepad"))
        ]
        processes = subprocess.run(["ps", "-eo", "pid,comm"], capture_output=True, text=True).stdout
        self.results["coverage"]["wine"] = {
            "wineboot_status": boot.returncode,
            "new_applications_on_the_bus": new,
            "wine_applications_on_the_bus": wine_like,
            "wine_processes": [line.strip() for line in processes.splitlines() if "wine" in line or "notepad" in line],
            "screenshot": shot,
        }
        self.checks.check("wine", "Wine's notepad was measured (whatever it exposes)", True, hard=False)
        self.session.stop("wine-notepad")
        wineserver = shutil.which("wineserver") or shutil.which("wineserver64")
        if wineserver:
            subprocess.run([wineserver, "-k"], env=self.session.environment(**environment), capture_output=True, timeout=60)

    def phase_stop(self):
        log = self.out / "mail-stop.log"
        log.write_text("")
        self.session.start("mail-stop", ["python3", HERE / "mail_app.py", log], GDK_BACKEND="wayland")
        if not self.wait_for_application("gtk3-stop", lambda names, app: "tur-a11y-mail" in names, 60):
            self.checks.check("stop", "the GTK 3 window came back for the stop test", False)
            return
        hidden = {}

        def hide(arrival):
            if arrival == 1 and not hidden:
                # The person switches P@x's indicator off while the pointer
                # rests on the button, before the press.
                started = time.monotonic()
                hidden["answer"] = gdbus(
                    self.session.environment(),
                    "--session",
                    "--dest",
                    "org.tur.PaxOverlay1",
                    "--object-path",
                    "/org/tur/PaxOverlay1",
                    "--method",
                    "org.tur.PaxOverlay1.Hide",
                )
                hidden["seconds"] = round(time.monotonic() - started, 3)

        record = self.run_task(
            "stop-while-acting",
            "Press Bold twice.",
            [
                {"calls": [
                    {"id": "t1", "name": "ui_press", "arguments": {"window": MAIL_TITLE, "name": "Bold"}},
                    {"id": "t2", "name": "ui_press", "arguments": {"window": MAIL_TITLE, "name": "Bold"}},
                ]},
                {"text": "Pressed twice."},
            ],
            on_arrival=hide,
        )
        self.results["notes"].append({"stop": hidden})
        self.checks.check("stop-while-acting", "the indicator was switched off during the task", bool(hidden), hidden)
        self.checks.check("stop-while-acting", "the task ended as cancelled", record["outcome"] == "cancelled", record["outcome"])
        self.checks.check("stop-while-acting", "with the code of a stop", record["error_code"] == "TUR-AI-INTERFACE-004", record["error_code"])
        self.checks.check("stop-while-acting", "the button was never pressed", "clicked:Bold" not in log.read_text().splitlines(), log.read_text())
        self.checks.check("stop-while-acting", "the second press was never decided", len(record["decisions"]) == 1, record["decisions"])
        self.session.stop("mail-stop")

    # ------------------------------------------------------------- driver

    def overlay_up(self):
        program = find_program(self.build, "tur-pax-overlay")
        if program is None:
            raise RuntimeError("tur-pax-overlay was not built")
        self.session.start("tur-pax-overlay", [program], QT_QPA_PLATFORM="wayland", QT_QUICK_BACKEND="software")
        if not self.session.wait_for(lambda: self.session.name_owned("org.tur.PaxOverlay1"), 30, step=0.5):
            raise RuntimeError("tur-pax-overlay did not take its name on the bus")
        # What the indicator alone looks like: the pill, with a lease held
        # and nothing pointed at. A screenshot counts as showing P@x's
        # pointer at an element only with clearly more gold than this.
        environment = self.session.environment()
        code, out, _ = gdbus(
            environment, "--session", "--dest", "org.tur.PaxOverlay1", "--object-path", "/org/tur/PaxOverlay1",
            "--method", "org.tur.PaxOverlay1.Acquire", "baseline",
        )
        lease = out.split("'")[1] if code == 0 and "'" in out else ""
        if lease:
            gdbus(
                environment, "--session", "--dest", "org.tur.PaxOverlay1", "--object-path", "/org/tur/PaxOverlay1",
                "--method", "org.tur.PaxOverlay1.SetActivity", lease, "acting", "{}",
            )
            time.sleep(2)
        shot = self.shooter.take("baseline-indicator")
        self.baseline_gold = shot.get("gold_pixels_sampled", 0)
        self.results["baseline_indicator"] = shot
        if lease:
            gdbus(
                environment, "--session", "--dest", "org.tur.PaxOverlay1", "--object-path", "/org/tur/PaxOverlay1",
                "--method", "org.tur.PaxOverlay1.Release", lease,
            )

    def run(self):
        status = 0
        try:
            self.session.up()
            self.results["accessibility_bus"] = self.session.a11y
            self.overlay_up()
            for phase in (self.phase_settings, self.phase_mail, self.phase_editor, self.phase_firefox, self.phase_wine, self.phase_stop):
                try:
                    phase()
                except Exception as error:  # noqa: BLE001 - one phase does not stop the others
                    self.checks.check(phase.__name__, "the phase ran to its end", False, repr(error))
        except Exception as error:  # noqa: BLE001
            self.checks.check("session", "the session started", False, repr(error))
        finally:
            self.session.down()
        self.results["checks"] = self.checks.items
        self.results["screenshots"] = self.shooter.taken
        (self.out / "results.json").write_text(json.dumps(self.results, indent=2))
        (self.out / "coverage.json").write_text(json.dumps(self.results["coverage"], indent=2))
        report(self.out, self.results)
        if self.checks.failed():
            status = 1
        print(f"\n{len(self.checks.items)} checks, {len(self.checks.failed())} hard failures", flush=True)
        return status


def tool_results(bodies):
    """The frames of the tool results, in order, from the last request the
    provider received (which carries the whole conversation)."""
    if not bodies:
        return []
    frames = []
    for message in bodies[-1].get("messages", []):
        content = message.get("content")
        if not isinstance(content, list):
            continue
        for block in content:
            if block.get("type") != "tool_result":
                continue
            text = block.get("content", [{}])[0].get("text", "{}")
            try:
                frames.append(json.loads(text).get("tur_pax_tool_result", {}))
            except json.JSONDecodeError:
                frames.append({"unparsed": text[:300]})
    return frames


def coverage_row(label, application):
    windows = application.get("windows", []) if application else []
    if not application:
        return f"| {label} | not on the bus | | | | | | |"
    elements = sum(window.get("elements", 0) for window in windows)
    named = sum(window.get("named", 0) for window in windows)
    positioned = sum(window.get("positioned", 0) for window in windows)
    editable = sum(window.get("editable", 0) for window in windows)
    interfaces = {}
    actions = {}
    for window in windows:
        for name, count in window.get("interfaces", {}).items():
            interfaces[name] = interfaces.get(name, 0) + count
        for name, count in window.get("actions", {}).items():
            actions[name] = actions.get(name, 0) + count
    top = ", ".join(f"{name} {count}" for name, count in sorted(actions.items(), key=lambda item: -item[1])[:6])
    faces = ", ".join(f"{name} {count}" for name, count in sorted(interfaces.items()))
    return (
        f"| {label} | {application.get('toolkit') or '?'} ({application.get('application')}) | {len(windows)} | {elements} | "
        f"{named} | {positioned} | {editable} | {faces} | {top} |"
    )


def report(out, results):
    lines = ["### P@x through the accessibility tree, under KWin", ""]
    lines.append(f"Accessibility bus: {results.get('accessibility_bus', {}).get('how', '?')}")
    lines.append("")
    lines.append("| Toolkit | On the bus as | Windows | Elements read | Named | With a position | Editable | Interfaces | Actions (most offered) |")
    lines.append("|---|---|---|---|---|---|---|---|---|")
    for label, key in [("Qt Quick: tur-settings", "qt"), ("GTK 3: test window", "gtk3"), ("GTK 4: gnome-text-editor", "gtk4"), ("Firefox", "firefox")]:
        lines.append(coverage_row(label, results["coverage"].get(key)))
    wine = results["coverage"].get("wine", {})
    lines.append(
        f"| Wine: notepad | {len(wine.get('wine_applications_on_the_bus', []))} application(s) on the bus that name Wine or notepad; "
        f"{len(wine.get('new_applications_on_the_bus', []))} new | | | | | | | |"
    )
    lines.append("")
    lines.append("| Task | Outcome | Decisions (class, decision) | Screenshots with gold |")
    lines.append("|---|---|---|---|")
    for name, record in results["scenarios"].items():
        decisions = ", ".join(f"{entry.get('TUR_AI_CLASS')} {entry.get('TUR_AI_DECISION')}" for entry in record["decisions"])
        baseline = results.get("baseline_indicator", {}).get("gold_pixels_sampled", 0)
        gold = sum(1 for shot in record["screenshots"] if shot.get("gold_pixels_sampled", 0) > baseline + 100)
        lines.append(f"| {name} | {record['outcome']} {record.get('error_code') or ''} | {decisions} | {gold} of {len(record['screenshots'])} |")
    lines.append("")
    failed = [item for item in results["checks"] if not item["ok"]]
    lines.append(f"{len(results['checks'])} checks; {len([item for item in failed if item['hard']])} hard failures; {len([item for item in failed if not item['hard']])} notes.")
    for item in failed:
        lines.append(f"- {'FAIL' if item['hard'] else 'note'} {item['scenario']}: {item['check']}: {item['detail'][:200]}")
    for note in results["notes"]:
        lines.append(f"- {json.dumps(note)[:400]}")
    (out / "report.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", required=True)
    parser.add_argument("--build", required=True)
    parser.add_argument("--kwin", required=True)
    parser.add_argument("--tur-pax", required=True)
    parser.add_argument("--bridge", required=True)
    arguments = parser.parse_args()
    return Harness(arguments).run()


if __name__ == "__main__":
    sys.exit(main())
