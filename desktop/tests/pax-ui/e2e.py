#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""P@x's window, tur-pax-ui, against the real P@x session service.

    e2e.py record --tur-pax PATH --work-dir DIR --evidence DIR
    e2e.py ui     --tur-pax PATH --work-dir DIR --evidence DIR --build DIR

Runs on the CI runner (the `pax-ui-end-to-end` job of the desktop workflow),
never on a development machine (AGENTS.md). The service runs exactly as the
`pax` workflow runs it (tests/pax/service_e2e.py, whose private session bus
this reuses): `tur-pax serve` started by a private dbus-daemon from an
activation file, the real Secret Service (gnome-keyring) for the key, the
read-only sandbox (bubblewrap), the test service's tools, the stand-in for
local AI, and the scripted provider (tests/pax/fake_provider.py). No real
model is called and no real key exists.

record   Asks the service everything tur-pax-ui's tests answer from, and
         lets it run the tasks they replay, and writes what it said, signal
         by signal, as the fixture of desktop/apps/tur-pax-ui/fixtures/agent1
         (into EVIDENCE/recorded/agent1). Then compares it with the committed
         fixture and fails on a difference other than times and identifiers:
         the fixture is what the service says, or the test fails.
         desktop/apps/tur-pax-ui/fixtures/update.py takes a run's recording.

ui       The end-to-end test. The service on a bus whose socket is under the
         checkout, so that the builder container (where tur-pax-ui was built,
         Fedora 45) can reach it; tur-pax-ui's QML test runner in the
         container, with tests/e2e/e2e_ask_stop.qml: P@x is asked, its steps
         appear as they happen, then a second task is stopped by the Stop
         button while its sandboxed command runs. Here: the service's own
         signals, the provider's requests, and that the command was killed.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tests" / "pax"))

import dbus  # noqa: E402
import dbus.mainloop.glib  # noqa: E402
from gi.repository import GLib  # noqa: E402

from fake_provider import ScriptedProvider  # noqa: E402
from service_e2e import KEY, KEYRING_PASSWORD, Bus, Checks, plain  # noqa: E402

AGENT = "org.tur.Agent1"
AGENT_PATH = "/org/tur/Agent1"
FIXTURE = ROOT / "desktop" / "apps" / "tur-pax-ui" / "fixtures" / "agent1"
STUB_AI_LOCAL = ROOT / "tests" / "pax" / "stub-tur-ai-local"
LICENCE = "SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0"

# What the scripted model does in each recorded task.
TWO_TOOLS = [
    {"text": "I will read the value and set it.",
     "calls": [{"id": "toolu_1", "name": "test_observe", "arguments": {}},
               {"id": "toolu_2", "name": "test_reversible", "arguments": {"value": "night"}}]},
    {"text": "The value is set for the night."},
]
REFUSED = [
    {"text": "I will read the value, then send the report.",
     "calls": [{"id": "toolu_1", "name": "test_observe", "arguments": {}},
               {"id": "toolu_2", "name": "test_consequential",
                "arguments": {"recipient": "marta@example.org", "subject": "Report"}}]},
    {"text": "I read the value. Sending the report needs your approval, which I cannot ask for here."},
]
STOP = [
    {"calls": [{"id": "toolu_1", "name": "run_readonly_command", "arguments": {"argv": ["/usr/bin/sleep", "45"]}}]},
    {"text": "Never asked for."},
]


def keypart(value):
    """An argument as part of a recorded reply's key (fixture/agent.cpp, keyPart)."""
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, dbus.UInt32)):
        return str(int(value))
    text = str(value)
    try:
        parsed = json.loads(text)
    except ValueError:
        return text
    if isinstance(parsed, dict):
        return json.dumps(parsed, separators=(",", ":"), sort_keys=True)
    return text


def document(value):
    """A value as the fixture keeps it: JSON text parsed."""
    if isinstance(value, str) and value[:1] in "{[":
        try:
            return json.loads(value)
        except ValueError:
            return value
    return value


class Service:
    """The service on a private bus, as tests/pax/service_e2e.py runs it."""

    def __init__(self, tur_pax, work, bus_directory):
        self.tur_pax = tur_pax
        self.work = work
        self.home = work / "home"
        self.runtime = Path(f"/run/user/{os.getuid()}")
        self.bus_directory = bus_directory
        self.bus = None
        self.provider = None
        self.signals = []
        self.lock = threading.Lock()

    def environment(self):
        return {
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "HOME": str(self.home),
            "XDG_CONFIG_HOME": str(self.home / ".config"),
            "XDG_STATE_HOME": str(self.home / ".local" / "state"),
            "XDG_DATA_HOME": str(self.home / ".local" / "share"),
            "XDG_RUNTIME_DIR": str(self.runtime),
            "DBUS_SESSION_BUS_ADDRESS": self.bus.address if self.bus else "",
            "LANG": "C.UTF-8",
        }

    def start(self):
        if self.work.exists():
            shutil.rmtree(self.work)
        if self.bus_directory.exists():
            shutil.rmtree(self.bus_directory)
        self.home.mkdir(parents=True)
        tools = self.work / "tools"
        for kind in ("manifests", "adapters"):
            (tools / kind).mkdir(parents=True)
        data = ROOT / "services" / "tur-pax" / "tests" / "data"
        shutil.copy(data / "test-service.manifest.json", tools / "manifests" / "test-service.json")
        shutil.copy(data / "test-service.adapters.json", tools / "adapters" / "test-service.json")
        self.provider = ScriptedProvider([], wire="anthropic").start()
        settings = self.home / ".config" / "tur" / "pax" / "settings.json"
        settings.parent.mkdir(parents=True)
        settings.write_text(json.dumps({
            "schema_version": 1, "enabled": False, "provider": "anthropic",
            "model": "scripted-model", "base_url": self.provider.base_url,
        }, indent=2))
        self.bus = Bus(self.work, "session", directory=self.bus_directory)
        models = ROOT / "services" / "tur-pax" / "data" / "models.json"
        (self.bus.services / f"{AGENT}.service").write_text(
            "[D-BUS Service]\n"
            f"Name={AGENT}\n"
            f"Exec={self.tur_pax} serve --manifests {tools / 'manifests'} --adapters {tools / 'adapters'}"
            f" --models {models} --ai-local {STUB_AI_LOCAL}\n"
        )
        self.bus.start(self.environment())
        for arguments in (["--unlock", "--components=secrets"], ["--start", "--components=secrets"]):
            subprocess.run(["gnome-keyring-daemon", *arguments], input=KEYRING_PASSWORD if "--unlock" in arguments else None,
                           env=self.environment(), capture_output=True, text=True, timeout=30)
        dbus.mainloop.glib.threads_init()
        dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
        self.listener = dbus.bus.BusConnection(self.bus.address)
        self.listener.add_signal_receiver(self.on_signal, dbus_interface=AGENT, path=AGENT_PATH, member_keyword="member")
        self.loop = GLib.MainLoop()
        threading.Thread(target=self.loop.run, daemon=True).start()
        self.client = dbus.bus.BusConnection(self.bus.address)
        # The bus starts the service when it is first called.
        self.client.call_blocking(AGENT, AGENT_PATH, "org.freedesktop.DBus.Introspectable", "Introspect", "", [], timeout=60)

    def on_signal(self, *arguments, member=None):
        with self.lock:
            self.signals.append({"member": str(member), "args": [document(plain(value)) for value in arguments], "at": time.time()})

    def stop(self):
        try:
            self.loop.quit()
        except Exception:  # noqa: BLE001
            pass
        if self.bus:
            self.bus.stop()
        if self.provider:
            self.provider.stop()
        subprocess.run(["pkill", "-u", str(os.getuid()), "-f", "gnome-keyring-daemon"], check=False)

    def call(self, method, signature="", *arguments, timeout=60):
        """(values, error name, error message)."""
        try:
            reply = self.client.call_blocking(AGENT, AGENT_PATH, AGENT, method, signature, list(arguments), timeout=timeout)
        except dbus.exceptions.DBusException as error:
            return None, error.get_dbus_name(), error.get_dbus_message()
        values = plain(reply)
        if values is None:
            values = []
        elif not isinstance(values, list) or signature_out_is_single(method):
            values = [values]
        return values, None, None

    def script(self, turns, repeat_last=False):
        with self.provider._lock:
            self.provider.turns = list(turns)
            self.provider.repeat_last = repeat_last
            self.provider._served = 0
            self.provider.requests.clear()

    def wait_for(self, found, timeout=30):
        deadline = time.time() + timeout
        while time.time() < deadline:
            value = found()
            if value:
                return value
            time.sleep(0.05)
        return None

    def signal_count(self):
        with self.lock:
            return len(self.signals)

    def signals_since(self, start):
        with self.lock:
            return [dict(item) for item in self.signals[start:]]


def signature_out_is_single(method):
    # Methods whose single answer is a list or a JSON array would otherwise
    # look like several values; all of org.tur.Agent1's answers but
    # GetConsentText's are one value.
    return method != "GetConsentText"


class Recorder:
    """Asks the service what tur-pax-ui's tests need and writes the fixture."""

    def __init__(self, service, out, checks):
        self.service = service
        self.out = out
        self.checks = checks
        self.replies = {}
        self.statuses = {}
        self.tasks = {}
        self.local = {}

    def record(self, method, signature="", *arguments, keep=True):
        values, error, message = self.service.call(method, signature, *arguments, timeout=120)
        key = " ".join([method] + [keypart(argument) for argument in arguments])
        if error:
            entry = {"error": error, "message": message}
        else:
            entry = {"reply": [document(value) for value in values]}
        if keep:
            self.replies[key] = entry
        return values, error

    def status(self, name):
        values, error = self.record("GetStatus", keep=False)
        self.checks.check("record", f"status {name}", error is None, error)
        self.statuses[name] = document(values[0]) if values else {}
        return self.statuses[name]

    def task(self, name, request, turns, stop_when=None, repeat_last=False):
        self.service.script(turns, repeat_last=repeat_last)
        start = self.service.signal_count()
        values, error = self.record("StartTask", "ss", request, "", keep=False)
        self.checks.check("record", f"task {name} started", error is None, error)
        task = values[0] if values else ""
        if stop_when:
            stop_when(task, start)
        finished = self.service.wait_for(lambda: [s for s in self.service.signals_since(start)
                                                  if s["member"] == "TaskFinished" and s["args"][0] == task], 60)
        self.checks.check("record", f"task {name} finished", bool(finished))
        time.sleep(0.5)
        recorded = [{"member": s["member"], "args": s["args"]} for s in self.service.signals_since(start)
                    if s["member"] != "LocalModelProgress"]
        self.tasks[name] = {"request": request, "signals": recorded}
        return finished[0]["args"][1] if finished else {}

    def local_model(self, model, options, until):
        start = self.service.signal_count()
        values, error = self.record("UseLocalModel", "ss", model, options)
        self.checks.check("record", f"local AI makes {model} ready", error is None, error)
        if until == "cancelled":
            self.service.wait_for(lambda: [s for s in self.service.signals_since(start) if s["member"] == "LocalModelProgress"], 20)
            self.record("CancelLocalModel")
        done = self.service.wait_for(lambda: [s for s in self.service.signals_since(start)
                                              if s["member"] == "LocalModelProgress" and s["args"][0] == model
                                              and s["args"][1].get("event") in (until, "failed")], 60)
        self.checks.check("record", f"local AI reported {until} for {model}", bool(done))
        time.sleep(0.3)
        self.local[model] = {"model": model, "signals": [{"member": s["member"], "args": s["args"]}
                                                         for s in self.service.signals_since(start)
                                                         if s["member"] == "LocalModelProgress" and s["args"][0] == model]}

    def run(self):
        service = self.service
        self.status("off")
        self.record("ListModels")
        self.record("ListLocalModels")
        for provider in ("anthropic", "deepseek", "openai", "local"):
            self.record("GetConsentText", "s", provider)
            self.record("ListProviderModels", "sb", provider, False)
        for provider, model in (("anthropic", "claude-opus-5-5"), ("openai", "gpt-6-luna"),
                                ("deepseek", "deepseek-flash"), ("local", "qwen3-0.6b")):
            self.record("GetGenerationSettings", "ss", provider, model)
        self.record("SetGenerationSettings", "sss", "openai", "gpt-6-luna", keypart('{"temperature": 0.3}'))
        self.record("SetGenerationSettings", "sss", "anthropic", "claude-opus-5-5", keypart('{"temperature": 0.3}'))
        self.record("HasKey", "s", "anthropic")
        self.record("Enable", "s", "1")
        version = self.replies["GetConsentText anthropic"]["reply"][0]
        _, error = self.record("Enable", "s", version, keep=False)
        self.checks.check("record", "P@x switched on with the consent text's version", error is None, error)
        self.status("no-key")
        self.record("SetKey", "ss", "anthropic", "two words")
        _, error = service.call("SetKey", "ss", "anthropic", KEY)
        self.checks.check("record", "the key is in the Secret Service", error is None, error)
        self.status("ready")

        result = self.task("two-tools", "Set the value for the night.", TWO_TOOLS)
        self.checks.check("record", "two-tools completed", result.get("outcome") == "completed", result.get("outcome"))
        result = self.task("refused", "Read the value and send the report to Marta.", REFUSED)
        refused = [step for step in result.get("steps", []) if step.get("decision") == "refused"]
        self.checks.check("record", "refused: the class 2 step was refused", len(refused) == 1, result.get("steps"))
        self.record("ListActivity", "u", dbus.UInt32(20))

        def cancel(task, start):
            proposed = service.wait_for(lambda: [s for s in service.signals_since(start)
                                                 if s["member"] == "ActionProposed" and s["args"][0] == task], 30)
            self.checks.check("record", "stop: the command was proposed", bool(proposed))
            time.sleep(1.0)
            service.call("CancelTask", "s", task)

        result = self.task("stop", "Wait for a while.", STOP, stop_when=cancel)
        self.checks.check("record", "stop: cancelled", result.get("outcome") == "cancelled", result.get("outcome"))

        today = self.statuses["ready"].get("spending", {}).get("today", {})
        used = today.get("input_tokens", 0) + today.get("output_tokens", 0)
        status = document(service.call("GetStatus")[0][0])
        today = status.get("spending", {}).get("today", {})
        used = today.get("input_tokens", 0) + today.get("output_tokens", 0)
        service.call("SetLimits", "s", json.dumps({"day_tokens": used + 100}))
        repeat = [{"text": "Again.", "calls": [{"id": "toolu_1", "name": "test_observe", "arguments": {}}]}]
        result = self.task("spend-limit", "Read the value again and again.", repeat, repeat_last=True)
        self.checks.check("record", "spend-limit: stopped at the day's limit", result.get("error_code") == "TUR-AI-SPEND-001", result.get("error_code"))
        self.status("limit-reached")
        service.call("SetLimits", "s", json.dumps({"day_tokens": None}))

        service.call("SetProvider", "ss", "anthropic", "claude-opus-5-5")
        result = self.task("priced", "Think about it.", [{"text": "I thought about it."}])
        self.checks.check("record", "priced: the task has a cost", (result.get("cost") or 0) > 0, result.get("cost"))
        self.status("priced")
        service.call("SetProvider", "ss", "anthropic", "scripted-model")

        self.record("UseLocalModel", "ss", "qwen3-0.6b", "")
        self.local_model("qwen3-0.6b", keypart('{"download_bytes": 639446688}'), "in-use")
        self.service.wait_for(lambda: document(service.call("GetStatus")[0][0]).get("local_model") is None, 20)
        self.status("local-consent")
        self.record("UseLocalModel", "ss", "ministral-3-3b", keypart('{"download_bytes": 2988524864}'))
        self.local_model("ministral-3-3b", keypart('{"accept_licence": true, "download_bytes": 2988524864}'), "in-use")
        self.service.wait_for(lambda: document(service.call("GetStatus")[0][0]).get("local_model") is None, 20)
        self.local_model("gemma-4-e2b", "{}", "cancelled")

    def write(self):
        root = self.out
        if root.exists():
            shutil.rmtree(root)
        for directory in ("status", "tasks", "local"):
            (root / directory).mkdir(parents=True)

        def dump(path, content):
            path.write_text(json.dumps({"licence": LICENCE, **content}, indent=2, sort_keys=True, ensure_ascii=False) + "\n")

        dump(root / "replies.json", {"replies": self.replies})
        for name, status in self.statuses.items():
            dump(root / "status" / f"{name}.json", {"recorded": status})
        for name, task in self.tasks.items():
            dump(root / "tasks" / f"{name}.json", task)
        for name, steps in self.local.items():
            dump(root / "local" / f"{name}.json", steps)


VOLATILE_KEYS = {"started_at", "finished_at", "time", "accepted_at", "recorded_at", "proposed_at", "fetched_at",
                 "day", "job"}
VOLATILE_TEXT = [
    (re.compile(r"\bt-\d{6,}-(\d+)"), r"t-*-\1"),
    (re.compile(r"\bl-\d+\b"), "l-*"),
    (re.compile(r"\b20\d\d-\d\d-\d\dT\d\d:\d\d:\d\dZ\b"), "*"),
]


def normalised(value, key=None):
    """A recorded value with its times and identifiers taken out."""
    if isinstance(value, dict):
        return {k: normalised(v, k) for k, v in value.items()}
    if isinstance(value, list):
        return [normalised(item) for item in value]
    if key in VOLATILE_KEYS and value is not None:
        return "*"
    if isinstance(value, str):
        for pattern, replacement in VOLATILE_TEXT:
            value = pattern.sub(replacement, value)
    return value


def differences(recorded, committed, where=""):
    out = []
    if type(recorded) is not type(committed) and not (isinstance(recorded, (int, float)) and isinstance(committed, (int, float))):
        return [f"{where or '/'}: {json.dumps(committed)[:120]} -> {json.dumps(recorded)[:120]}"]
    if isinstance(recorded, dict):
        for key in sorted(set(recorded) | set(committed)):
            if key not in committed:
                out.append(f"{where}/{key}: new")
            elif key not in recorded:
                out.append(f"{where}/{key}: gone")
            else:
                out += differences(recorded[key], committed[key], f"{where}/{key}")
    elif isinstance(recorded, list):
        if len(recorded) != len(committed):
            out.append(f"{where}: {len(committed)} items -> {len(recorded)}")
        for index, (a, b) in enumerate(zip(recorded, committed)):
            out += differences(a, b, f"{where}[{index}]")
    elif recorded != committed:
        out.append(f"{where}: {json.dumps(committed)[:120]} -> {json.dumps(recorded)[:120]}")
    return out


def compare_with_committed(recorded_root, checks, evidence):
    report = []
    files = sorted(path.relative_to(recorded_root) for path in recorded_root.rglob("*.json"))
    committed_files = sorted(path.relative_to(FIXTURE) for path in FIXTURE.rglob("*.json")) if FIXTURE.exists() else []
    for gone in sorted(set(committed_files) - set(files)):
        report.append(f"{gone}: committed, not recorded")
    for relative in files:
        recorded = normalised(json.loads((recorded_root / relative).read_text()))
        path = FIXTURE / relative
        if not path.exists():
            report.append(f"{relative}: recorded, not committed")
            continue
        committed = normalised(json.loads(path.read_text()))
        found = differences(recorded, committed)
        report += [f"{relative}{line}" for line in found[:40]]
    (evidence / "fixture-differences.txt").write_text("\n".join(report) + "\n")
    checks.check("fixture", "the committed fixture is what the service says (desktop/apps/tur-pax-ui/fixtures/update.py RUN_ID)",
                 not report, "\n".join(report[:60]))


def run_record(arguments, checks):
    evidence = arguments.evidence
    service = Service(arguments.tur_pax, arguments.work_dir / "record", arguments.work_dir / "record-bus")
    try:
        service.start()
        recorder = Recorder(service, evidence / "recorded" / "agent1", checks)
        recorder.run()
        recorder.write()
    finally:
        service.stop()
    compare_with_committed(evidence / "recorded" / "agent1", checks, evidence)


def run_ui(arguments, checks):
    evidence = arguments.evidence
    # The socket must be where the container can reach it: under the checkout,
    # which in-builder.sh mounts at the same path.
    bus_directory = ROOT / "out" / "desktop" / "pax-ui-e2e-bus"
    service = Service(arguments.tur_pax, arguments.work_dir / "ui", bus_directory)
    seen_sleep = threading.Event()
    stop_watching = threading.Event()

    def watch_sleep():
        while not stop_watching.is_set():
            found = subprocess.run(["pgrep", "-u", str(os.getuid()), "-f", "^/usr/bin/sleep 45"], capture_output=True)
            if found.returncode == 0:
                seen_sleep.set()
            time.sleep(0.2)

    try:
        service.start()
        version = service.call("GetConsentText", "s", "anthropic")[0][0]
        _, error, message = service.call("Enable", "s", version)
        checks.check("ui", "P@x is switched on for the test", error is None, message)
        _, error, message = service.call("SetKey", "ss", "anthropic", KEY)
        checks.check("ui", "the key is in the Secret Service", error is None, message)
        service.script(TWO_TOOLS + STOP)
        watcher = threading.Thread(target=watch_sleep, daemon=True)
        watcher.start()
        shots = evidence / "e2e-shots"
        shots.mkdir(parents=True, exist_ok=True)
        build = arguments.build.resolve()
        command = [
            str(ROOT / "desktop" / "ci" / "in-builder.sh"), "env",
            f"TUR_PAX_UI_E2E_BUS={service.bus.address}",
            "QT_QPA_PLATFORM=offscreen", "QT_QUICK_BACKEND=software", "QT_FONT_DPI=96", "QT_SCALE_FACTOR=1",
            f"QML_IMPORT_PATH={build / 'qml'}", f"XDG_CONFIG_HOME={evidence / 'empty-config'}",
            f"XDG_CONFIG_DIRS={evidence / 'empty-config'}", "LC_ALL=C.UTF-8",
            f"TUR_SCREENSHOT_DIR={shots}",
            str(build / "apps" / "tur-pax-ui" / "tests" / "tst_paxui"),
            "-input", str(ROOT / "desktop" / "apps" / "tur-pax-ui" / "tests" / "e2e" / "e2e_ask_stop.qml"),
        ]
        started = time.time()
        ran = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=600)
        (evidence / "e2e-qml.log").write_text(ran.stdout + ran.stderr)
        print(ran.stdout[-6000:] + ran.stderr[-3000:], flush=True)
        checks.check("ui", "tur-pax-ui's end-to-end QML test passed in the builder container", ran.returncode == 0,
                     f"exit {ran.returncode}; see e2e-qml.log")
        stop_watching.set()
        signals = service.signals_since(0)
        tasks = [s for s in signals if s["member"] == "TaskFinished" and s["at"] >= started]
        outcomes = [s["args"][1].get("outcome") for s in tasks]
        checks.check("ui", "the service ran two tasks for the window: one completed, one cancelled",
                     outcomes == ["completed", "cancelled"], outcomes)
        if len(tasks) == 2:
            first = tasks[0]["args"][1]
            checks.check("ui", "the first ran both of its steps", [step["state"] for step in first.get("steps", [])] == ["done", "done"],
                         first.get("steps"))
            second = tasks[1]["args"][1]
            checks.check("ui", "the second was cancelled with its code", second.get("error_code") == "TUR-AI-TASK-003",
                         second.get("error_code"))
            streamed = [s["member"] for s in signals if s["args"] and s["args"][0] == tasks[0]["args"][0]]
            checks.check("ui", "the first task's steps were streamed as signals",
                         streamed.count("ActionProposed") == 2 and streamed.count("ActionCompleted") == 2, streamed)
        checks.check("ui", "the stopped task's sandboxed command was running", seen_sleep.is_set())
        gone = service.wait_for(lambda: subprocess.run(["pgrep", "-u", str(os.getuid()), "-f", "^/usr/bin/sleep 45"],
                                                       capture_output=True).returncode != 0, 10)
        checks.check("ui", "and was killed, not left to run", gone)
        checks.check("ui", "the provider was asked three times: two answers, then the stopped task's first",
                     len(service.provider.requests) == 3, len(service.provider.requests))
        sent = json.dumps(service.provider.bodies())
        checks.check("ui", "the window's words reached the model", "Set the value for the night." in sent)
        (evidence / "e2e-signals.json").write_text(json.dumps(signals, indent=2))
    finally:
        stop_watching.set()
        service.stop()
        shutil.rmtree(bus_directory, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("phase", choices=["record", "ui"])
    parser.add_argument("--tur-pax", required=True, type=Path)
    parser.add_argument("--work-dir", required=True, type=Path)
    parser.add_argument("--evidence", required=True, type=Path)
    parser.add_argument("--build", type=Path, default=ROOT / "desktop" / "build" / "ci")
    arguments = parser.parse_args()
    arguments.tur_pax = arguments.tur_pax.resolve()
    arguments.work_dir = arguments.work_dir.resolve()
    arguments.evidence = arguments.evidence.resolve()
    arguments.evidence.mkdir(parents=True, exist_ok=True)
    checks = Checks()
    try:
        if arguments.phase == "record":
            run_record(arguments, checks)
        else:
            run_ui(arguments, checks)
    except Exception:  # noqa: BLE001
        import traceback
        checks.check("harness", "the test ran to its end", False, traceback.format_exc())
    failed = checks.failed()
    (arguments.evidence / f"{arguments.phase}-checks.json").write_text(json.dumps(checks.results, indent=2))
    lines = [f"### P@x's window against the real service: {arguments.phase}", "",
             f"{len(checks.results) - len(failed)} checks passed, {len(failed)} failed.", ""]
    lines += [f"- {'pass' if r['passed'] else 'FAIL'}: {r['check']}" for r in checks.results]
    (arguments.evidence / f"{arguments.phase}-summary.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines), flush=True)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
