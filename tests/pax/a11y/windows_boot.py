#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""P@x operating a Windows application on the installed TUR system
(docs/architecture/pax-accessibility.md, section 11; docs/reports/
pax-winbridge-1.md).

Run by the boot test's Windows check (tests/boot/guest/rootfs/usr/libexec/
tur-boot-test/windows-check) as the test user, in the signed-in Plasma
session, SELinux enforcing, after tur-apps has installed 7-Zip from the
catalogue (org.7-zip.7zip) with the image's Wine:

  1. 7-Zip is started as its launcher entry starts it (`tur-runtime launch
     PROFILE`, with its own folder to open): inside its sandbox, with P@x's bridge offered on
     $XDG_RUNTIME_DIR/tur/winbridge/org.7-zip.7zip.sock;
  2. P@x's real task loop (`tur-pax run --interface`, the policy engine,
     tur-pax-a11y, the overlay if the image has it) with a scripted
     provider (tests/pax/fake_provider.py; no model is called): reads the
     window, selects readme.txt, types into the address field, is refused
     Delete, presses Info;
  3. the rule for a position proposed from a picture, on the real window
     (`tur-pax-a11y confirm-point`): a point on Delete proposed as Info is
     refused, as Delete confirmed;
  4. 7-Zip is closed the way a person closes it; the socket must be gone.

Screenshots are the host's (--shot-fifo): one when 7-Zip is up, one each
time P@x's pointer arrives at a control (the bridge's log line), one after
each task. Writes EVIDENCE/results.json and results.md, each task's
transcript, audit trail and bridge log. Exit status 0 when it ran; the
checks are in results.json, each marked required or not.

    windows_boot.py --app org.7-zip.7zip --evidence DIR [--shot-fifo FIFO]
"""

import argparse
import json
import os
import pathlib
import re
import shutil
import socket
import subprocess
import sys
import threading
import time

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from fake_provider import ScriptedProvider  # noqa: E402

KEY = "sk-test-not-a-real-key-winbridge"


class Run:
    def __init__(self, arguments):
        self.arguments = arguments
        self.evidence = pathlib.Path(arguments.evidence).resolve()
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.checks = []
        self.tasks = {}
        self.runtime_dir = pathlib.Path(os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}"))
        self.socket = self.runtime_dir / "tur" / "winbridge" / f"{arguments.app}.sock"
        self.shot_count = 0
        self.no_tools = self.evidence / "no-tools"
        self.no_tools.mkdir(exist_ok=True)

    def log(self, text):
        print(text, flush=True)

    def check(self, name, ok, detail="", required=True):
        self.checks.append({"check": name, "ok": bool(ok), "required": required, "detail": str(detail)[:800]})
        self.log(f"  [{'ok  ' if ok else ('FAIL' if required else 'note')}] {name}" + ("" if ok else f" -- {str(detail)[:300]}"))

    def shot(self, name):
        self.shot_count += 1
        name = re.sub(r"[^A-Za-z0-9._-]", "-", f"pax-{self.shot_count:02d}-{name}")
        if self.arguments.shot_fifo:
            with open(self.arguments.shot_fifo, "w") as fifo:
                fifo.write(f"SHOT {name}\n")
        return name

    def run_tool(self, command, timeout=120):
        done = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
        return done.returncode, done.stdout, done.stderr

    # ------------------------------------------------------------ the bridge, directly

    def ask_bridge(self, requests, timeout=120):
        answers = []
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
            connection.settimeout(timeout)
            connection.connect(str(self.socket))
            reader = connection.makefile("rb")
            for number, request in enumerate(requests, start=1):
                connection.sendall((json.dumps(dict(request, id=number)) + "\n").encode())
                while True:
                    line = reader.readline()
                    if not line:
                        raise RuntimeError("the bridge ended")
                    try:
                        reply = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    if reply.get("id") == number:
                        answers.append(reply)
                        break
        return answers

    # ------------------------------------------------------------ one task

    def task(self, name, request, turns):
        self.log(f"\n== task {name}: {request}")
        provider = ScriptedProvider(turns).start()
        config = {
            "schema_version": 1, "provider": "anthropic", "base_url": provider.base_url, "model": "scripted-model",
            "max_output_tokens": 1024, "timeouts": {"connect_ms": 5000, "response_ms": 120000, "idle_ms": 120000},
            "retry": {"max_attempts": 1, "initial_delay_ms": 10, "max_delay_ms": 100},
        }
        config_path = self.evidence / f"{name}.provider.json"
        config_path.write_text(json.dumps(config, indent=2))
        audit_path = self.evidence / f"{name}.audit.jsonl"
        audit_path.unlink(missing_ok=True)
        command = [
            self.arguments.tur_pax, "run", request, "--json", "--provider-config", str(config_path), "--interface",
            "--interface-bridge", self.arguments.bridge, "--no-commands", "--tools", str(self.no_tools),
            "--audit-log", str(audit_path), "--task", name, "--max-seconds", "400", "--max-steps", "12",
        ]
        environment = dict(os.environ, TUR_PAX_API_KEY=KEY)
        process = subprocess.Popen(command, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        lines, shots = [], []

        def follow():
            for line in process.stderr:
                lines.append(line.rstrip("\n"))
                if "TUR-A11Y arrived=" in line:
                    shots.append(self.shot(f"{name}-arrived"))

        follower = threading.Thread(target=follow, daemon=True)
        follower.start()
        bound = threading.Timer(600, process.kill)
        bound.start()
        stdout = process.stdout.read()
        process.wait()
        bound.cancel()
        follower.join(timeout=30)
        provider.stop()
        time.sleep(1)
        shots.append(self.shot(f"{name}-after"))
        try:
            transcript = json.loads(stdout) if stdout.strip() else {}
        except json.JSONDecodeError:
            transcript = {"unparsed": stdout[:4000]}
        audit = [json.loads(line) for line in audit_path.read_text().splitlines() if line.strip()] if audit_path.exists() else []
        (self.evidence / f"{name}.transcript.json").write_text(json.dumps(transcript, indent=2))
        (self.evidence / f"{name}.stderr.txt").write_text("\n".join(lines) + "\n")
        (self.evidence / f"{name}.requests.json").write_text(json.dumps(provider.bodies(), indent=2))
        results = []
        bodies = provider.bodies()
        if bodies:
            for message in bodies[-1].get("messages", []):
                content = message.get("content")
                if isinstance(content, list):
                    for block in content:
                        if block.get("type") == "tool_result":
                            text = block.get("content", [{}])[0].get("text", "{}")
                            try:
                                results.append(json.loads(text).get("tur_pax_tool_result", {}))
                            except json.JSONDecodeError:
                                results.append({"unparsed": text[:300]})
        record = {
            "request": request,
            "status": process.returncode,
            "outcome": transcript.get("outcome"),
            "error_code": transcript.get("error_code"),
            "decisions": [entry for entry in audit if entry.get("TUR_AI_EVENT") == "decision"],
            "executed": [entry for entry in audit if entry.get("TUR_AI_EVENT") == "executed"],
            "results": results,
            "screenshots": shots,
            "bridge_log": [line for line in lines if "TUR-A11Y" in line],
        }
        self.tasks[name] = record
        events = [entry.get("TUR_AI_EVENT") for entry in audit]
        seen_decision, order_ok = False, True
        for event in events:
            seen_decision = seen_decision or event == "decision"
            if event == "executed" and not seen_decision:
                order_ok = False
        self.check(f"{name}: nothing was performed before a decision", order_ok, events)
        above = [entry for entry in record["decisions"] if int(entry.get("TUR_AI_CLASS", "0")) >= 2]
        self.check(f"{name}: no decision above class 1 is anything but refused",
                   all(entry.get("TUR_AI_DECISION") == "refused" for entry in above), above)
        return record

    # ------------------------------------------------------------ the run

    def main(self):
        arguments = self.arguments
        overlay = shutil.which("tur-pax-overlay") is not None or pathlib.Path(
            "/usr/share/dbus-1/services/org.tur.PaxOverlay1.service").exists()
        environment = {
            "tur-pax-a11y": self.run_tool([arguments.bridge, "--version"])[1].strip(),
            "overlay_in_the_image": overlay,
            "winbridge": sorted(str(path) for path in pathlib.Path("/usr/lib/tur/winbridge").glob("*")),
            "XDG_RUNTIME_DIR": str(self.runtime_dir),
            "DISPLAY": os.environ.get("DISPLAY"),
            "WAYLAND_DISPLAY": os.environ.get("WAYLAND_DISPLAY"),
            "selinux": self.run_tool(["getenforce"])[1].strip(),
            "context": self.run_tool(["id", "-Z"])[1].strip(),
        }
        self.log(json.dumps(environment, indent=2))
        code, out, err = self.run_tool([arguments.tur_runtime, "app", "show", arguments.app, "--json"])
        try:
            shown = json.loads(out)
        except json.JSONDecodeError:
            shown = {}
        record = shown.get("record", shown)
        profile = record.get("profile")
        self.check("the application is installed (tur-runtime app show)", code == 0 and profile, err[-400:] or out[-400:])
        if not profile:
            return self.finish(environment)
        prefix = pathlib.Path(record.get("prefix", ""))
        code, out, err = self.run_tool([arguments.tur_runtime, "launch", profile, "--dry-run", "--json"])
        try:
            plan = json.loads(out).get("launch", {})
        except json.JSONDecodeError:
            plan = {}
        (self.evidence / "launch-plan.json").write_text(json.dumps(plan, indent=2))
        offer = plan.get("winbridge") or {}
        self.check("the launch plan offers P@x's bridge on the application's socket, menu hook allowed (not a game)",
                   offer.get("socket") == str(self.socket) and offer.get("menu_hook") is True, offer or err[-400:])

        # 1. 7-Zip, as its launcher entry starts it.
        launch_log = open(self.evidence / "launch.log", "wb")
        # 7-Zip's file manager opens the folder it is given: its own, so
        # that the rows are known (readme.txt is one).
        launcher = subprocess.Popen([arguments.tur_runtime, "launch", profile, "--", "C:\\Program Files\\7-Zip\\"],
                                    stdout=launch_log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
        launch_log.close()
        windows, deadline = [], time.monotonic() + 180
        while time.monotonic() < deadline and not windows:
            time.sleep(3)
            if not self.socket.exists():
                continue
            try:
                answer = self.ask_bridge([{"op": "hello"}, {"op": "windows"}])
                hello = answer[0].get("result", {})
                windows = [w for w in answer[1].get("result", {}).get("windows", []) if (w.get("exe") or "").lower() == "7zfm.exe"
                           and w.get("screen", [0, 0, 0, 0])[2] > 100]
            except (OSError, RuntimeError) as error:
                hello = {"error": str(error)}
        (self.evidence / "bridge-hello.json").write_text(json.dumps({"hello": hello, "windows": windows}, indent=2))
        mode = oct(self.socket.stat().st_mode & 0o777) if self.socket.exists() else None
        self.check("the socket is there while 7-Zip runs, mode 0600", mode == "0o600", mode)
        self.check("the bridge started inside 7-Zip's sandbox sees 7-Zip's window", bool(windows), hello)
        self.check("the bridge is the image's, with the menu hook allowed", hello.get("hook_allowed") is True
                   and hello.get("bridge") == "tur-winbridge", hello)
        if not windows:
            launcher.terminate()
            return self.finish(environment)
        title = windows[0].get("title", "")
        time.sleep(3)
        self.shot("7zip-started")

        window = arguments.app
        # 2. P@x: read, select a row, type into the address field.
        record = self.task(
            "pax-7zip-read-select-type",
            "In 7-Zip, read the window, select readme.txt and type hello into the address field.",
            [
                {"calls": [{"id": "t1", "name": "ui_list_windows", "arguments": {}}]},
                {"calls": [{"id": "t2", "name": "ui_read_window", "arguments": {"window": window, "max_nodes": 120}}]},
                {"calls": [{"id": "t3", "name": "ui_select", "arguments": {"window": window, "name": "readme.txt"}}]},
                {"calls": [{"id": "t4", "name": "ui_type", "arguments": {"window": window, "text": "hello"}}]},
                {"text": "Done."},
            ],
        )
        decisions = [(entry.get("TUR_AI_CLASS"), entry.get("TUR_AI_DECISION")) for entry in record["decisions"]]
        results = record["results"]
        self.check("read and select: classes 0, 0, 1, 1, all allowed",
                   decisions == [("0", "allowed"), ("0", "allowed"), ("1", "allowed"), ("1", "allowed")], decisions)
        listed = results[0].get("output", {}).get("windows", []) if results else []
        ours = [w for w in listed if w.get("application") == arguments.app]
        self.check("the window list has 7-Zip as org.7-zip.7zip, toolkit win32 (Wine), owner user",
                   len(ours) >= 1 and ours[0].get("toolkit") == "win32 (Wine)" and ours[0].get("owner") == "user", listed)
        read = results[1].get("output", {}) if len(results) > 1 else {}
        (self.evidence / "read.json").write_text(json.dumps(read, indent=2))
        text = json.dumps(read)
        self.check("the read has 7-Zip's toolbar buttons, its menus and its rows",
                   all(f'"{name}"' in text for name in ("Add", "Extract", "Delete", "Info", "File", "readme.txt")), text[:600])
        statuses = [result.get("status") for result in results]
        self.check("selecting and typing were performed", statuses[2:4] == ["ok", "ok"], results[2:4],
                   required=overlay)
        typed = results[3].get("output", {}) if len(results) > 3 else {}
        self.check("the address field holds hello, read back by the bridge", typed.get("text_now_contains_typed_text") is True,
                   typed, required=overlay)
        self.check("P@x's pointer arrived at the controls it acted on", sum("arrived=true" in line for line in record["bridge_log"]) >= 2,
                   record["bridge_log"], required=False)

        # 3. Delete: class 2, refused; nothing reaches 7-Zip.
        readme = prefix / "drive_c" / "Program Files" / "7-Zip" / "readme.txt"
        existed = readme.exists()
        record = self.task(
            "pax-7zip-delete-refused",
            "Delete the selected file in 7-Zip.",
            [{"calls": [{"id": "t1", "name": "ui_press", "arguments": {"window": window, "name": "Delete"}}]},
             {"text": "That needs your approval."}],
        )
        decisions = [(entry.get("TUR_AI_CLASS"), entry.get("TUR_AI_DECISION"), entry.get("TUR_ERROR_CODE")) for entry in record["decisions"]]
        self.check("Delete: class 2, refused (TUR-AI-POLICY-006)", decisions == [("2", "refused", "TUR-AI-POLICY-006")], decisions)
        self.check("Delete: nothing was performed and the bridge never pointed", not record["executed"] and not record["bridge_log"],
                   [record["executed"], record["bridge_log"]])
        self.check("readme.txt is still in 7-Zip's folder", existed and readme.exists(), f"{readme}: before {existed}, after {readme.exists()}")

        # 4. A harmless button: Info, the properties of the selected file.
        record = self.task(
            "pax-7zip-press-info",
            "Show the properties of the selected file in 7-Zip.",
            [{"calls": [{"id": "t1", "name": "ui_press", "arguments": {"window": window, "name": "Info"}}]},
             {"text": "The properties are shown."}],
        )
        decisions = [(entry.get("TUR_AI_CLASS"), entry.get("TUR_AI_DECISION")) for entry in record["decisions"]]
        self.check("Info: class 1, allowed", decisions == [("1", "allowed")], decisions)
        self.check("Info was pressed", record["results"] and record["results"][0].get("status") == "ok", record["results"],
                   required=overlay)
        time.sleep(3)
        after = self.ask_bridge([{"op": "windows"}])[0].get("result", {}).get("windows", [])
        (self.evidence / "windows-after-info.json").write_text(json.dumps(after, indent=2))
        self.check("7-Zip opened a window for the properties", len([w for w in after if (w.get("exe") or "").lower() == "7zfm.exe"
                                                                    and w.get("screen", [0, 0, 0, 0])[2] > 100]) >= 2,
                   [w.get("title") for w in after], required=overlay)

        # 5. The rule for a position proposed from a picture.
        delete = None
        for element in self.flatten(read.get("elements", [])):
            if element.get("name") == "Delete" and element.get("bounds"):
                delete = element
        if delete:
            x, y, w, h = delete["bounds"]
            points = {}
            for asked in ("Info", "Delete"):
                code, out, err = self.run_tool([arguments.bridge, "confirm-point", title, str(x + w // 2), str(y + h // 2), asked])
                try:
                    points[asked] = json.loads(out)
                except json.JSONDecodeError:
                    points[asked] = {"status": code, "stderr": err[-400:]}
            (self.evidence / "confirm-point.json").write_text(json.dumps(points, indent=2))
            info = points.get("Info", {})
            self.check("a point on Delete proposed as Info is refused, class 2, naming Delete",
                       info.get("class") == 2 and (info.get("confirmation", {}).get("under_the_point") or {}).get("name") == "Delete", info)
            self.check("the same point proposed as Delete is confirmed (then decided as any press of Delete: class 2)",
                       points.get("Delete", {}).get("confirmation", {}).get("decision") == "confirmed", points.get("Delete"),
                       required=False)
        else:
            self.check("the read gave Delete a position", False, "no bounds")

        # 6. Closed the way a person closes it; the bridge and the socket go.
        for _ in range(3):
            subprocess.run(["wmctrl", "-c", title], capture_output=True, timeout=20)
            subprocess.run(["wmctrl", "-c", "Properties"], capture_output=True, timeout=20)
            time.sleep(2)
            if launcher.poll() is not None:
                break
        try:
            launcher.wait(timeout=90)
        except subprocess.TimeoutExpired:
            launcher.terminate()
            launcher.wait(timeout=30)
        self.check("7-Zip ended when its window was closed", launcher.returncode is not None, launcher.returncode, required=False)
        time.sleep(2)
        self.check("the socket is removed when 7-Zip has ended", not self.socket.exists(), str(self.socket), required=False)
        return self.finish(environment)

    @staticmethod
    def flatten(elements):
        for element in elements:
            yield element
            yield from Run.flatten(element.get("children", []))

    def finish(self, environment):
        failed = [c for c in self.checks if c["required"] and not c["ok"]]
        document = {"environment": environment, "checks": self.checks, "tasks": self.tasks,
                    "failed_required": [c["check"] for c in failed]}
        (self.evidence / "results.json").write_text(json.dumps(document, indent=2))
        lines = ["## P@x operating a Windows application (7-Zip) on the installed system", "",
                 f"Overlay in the image: {environment.get('overlay_in_the_image')}; SELinux {environment.get('selinux')}; "
                 f"{environment.get('tur-pax-a11y')}", "",
                 "| Check | Required | Result | Detail |", "|---|---|---|---|"]
        for c in self.checks:
            lines.append(f"| {c['check']} | {'yes' if c['required'] else 'no'} | {'pass' if c['ok'] else '**FAIL**'} | "
                         f"{c['detail'].replace('|', '/').replace(chr(10), ' ')[:300]} |")
        lines += ["", "| Task | Outcome | Decisions (class, decision) | Screenshots |", "|---|---|---|---|"]
        for name, record in self.tasks.items():
            decisions = ", ".join(f"{e.get('TUR_AI_CLASS')} {e.get('TUR_AI_DECISION')}" for e in record["decisions"])
            lines.append(f"| {name} | {record['outcome']} {record.get('error_code') or ''} | {decisions} | {', '.join(record['screenshots'])} |")
        (self.evidence / "results.md").write_text("\n".join(lines) + "\n")
        self.log("\n".join(lines))
        return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--app", required=True)
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--shot-fifo")
    parser.add_argument("--tur-pax", default="/usr/bin/tur-pax")
    parser.add_argument("--bridge", default="/usr/bin/tur-pax-a11y")
    parser.add_argument("--tur-runtime", default="/usr/bin/tur-runtime")
    return Run(parser.parse_args()).main()


if __name__ == "__main__":
    sys.exit(main())
