#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The Windows path under the real task loop and the real policy engine
(winsee experiment, docs/reports/exp-winsee-1.md).

`tur-pax run --interface --interface-bridge adapter.py`: the task loop of
tur-pax, its registry of the eleven ui_* tools, its policy engine and its
audit trail, unchanged, with the experiment's adapter where tur-pax-a11y
would be and tur-winbridge.exe inside the Wine prefix behind it. A scripted
provider (tests/pax/fake_provider.py) says what the model "does"; no model
is called. Windows programs: TUR's own test windows (tw-controls.exe,
tw-attack.exe) and Wine's notepad.

It checks that the same tools give the same classes on this path (reading
class 0, ordinary acting class 1, a control named Delete or a menu item
named Replace class 2 and refused, a password field and a line break
refused), that every action was decided before it was performed, and that
what was refused did not reach the application. Then it speaks to the
adapter directly, as a faulty task loop would: a target that is not what
the application shows now is `changed`; a target the engine would refuse
is `refused` by the bridge's own decision.

    policy.py --tur-pax PATH --decide PATH --build DIR --work DIR --evidence DIR --repository DIR

Runs on the CI runner inside a Fedora container, as an ordinary user.
"""

import argparse
import json
import os
import pathlib
import subprocess
import sys
import time
import traceback

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from bridge import windows_path, wine_environment  # noqa: E402
from common import Display  # noqa: E402

KEY = "sk-test-not-a-real-key-winsee"
CONTROLS = "TUR controls test"
ATTACK = "Clean up"


def tool_results(bodies):
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


class Policy:
    def __init__(self, arguments):
        self.arguments = arguments
        self.build = pathlib.Path(arguments.build).resolve()
        self.work = pathlib.Path(arguments.work).resolve()
        self.evidence = pathlib.Path(arguments.evidence).resolve() / "policy"
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.repository = pathlib.Path(arguments.repository).resolve()
        sys.path.insert(0, str(self.repository / "tests" / "pax"))
        self.prefix = self.work / "prefix"
        self.display = Display(self.evidence / "logs", number=97)
        self.env = wine_environment(self.prefix, self.display.name)
        self.adapter_log = self.evidence / "adapter.jsonl"
        self.checks = []
        self.tasks = {}
        self.processes = []
        self.empty_tools = self.work / "no-tools"
        self.empty_tools.mkdir(parents=True, exist_ok=True)

    def check(self, scenario, what, ok, detail=""):
        self.checks.append({"scenario": scenario, "check": what, "ok": bool(ok), "detail": str(detail)[:500]})
        print(f"  [{'ok  ' if ok else 'FAIL'}] {scenario}: {what}" + (f" -- {str(detail)[:300]}" if not ok else ""), flush=True)

    def pax_environment(self):
        environment = dict(self.env)
        home = self.work / "pax-home"
        for name in ("config", "data", "cache", "runtime"):
            (home / name).mkdir(parents=True, exist_ok=True)
        (home / "runtime").chmod(0o700)
        environment.update({
            "HOME": str(home), "XDG_CONFIG_HOME": str(home / "config"), "XDG_DATA_HOME": str(home / "data"), "XDG_CACHE_HOME": str(home / "cache"),
            "XDG_RUNTIME_DIR": os.environ.get("XDG_RUNTIME_DIR", str(home / "runtime")),
            "TUR_PAX_API_KEY": KEY,
            "WINSEE_BRIDGE": str(self.build / "tur-winbridge.exe"), "WINSEE_DECIDE": str(pathlib.Path(self.arguments.decide).resolve()),
            "WINSEE_LOG": str(self.adapter_log), "WINSEE_BRIDGE_LOG": str(self.evidence / "logs" / "bridge.log"),
        })
        if "DBUS_SESSION_BUS_ADDRESS" in os.environ:
            environment["DBUS_SESSION_BUS_ADDRESS"] = os.environ["DBUS_SESSION_BUS_ADDRESS"]
        return environment

    def launch(self, name, command):
        log = open(self.evidence / "logs" / f"{name}.log", "ab")
        process = subprocess.Popen(["wine", *[str(part) for part in command]], env=self.env, stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
        log.close()
        self.processes.append(process)
        return process

    def run_task(self, name, request, turns):
        from fake_provider import ScriptedProvider

        print(f"\n== task {name}: {request}", flush=True)
        provider = ScriptedProvider(turns).start()
        config = {
            "schema_version": 1, "provider": "anthropic", "base_url": provider.base_url, "model": "scripted-model", "max_output_tokens": 1024,
            "timeouts": {"connect_ms": 5000, "response_ms": 60000, "idle_ms": 60000},
            "retry": {"max_attempts": 1, "initial_delay_ms": 10, "max_delay_ms": 100},
        }
        config_path = self.evidence / f"{name}.provider.json"
        config_path.write_text(json.dumps(config, indent=2))
        audit_path = self.evidence / f"{name}.audit.jsonl"
        if audit_path.exists():
            audit_path.unlink()
        command = [
            self.arguments.tur_pax, "run", request, "--json", "--provider-config", str(config_path), "--interface", "--interface-bridge",
            str(HERE / "adapter.py"), "--no-commands", "--tools", str(self.empty_tools), "--audit-log", str(audit_path), "--task", name,
            "--max-seconds", "240", "--max-steps", "12",
        ]
        try:
            done = subprocess.run(command, env=self.pax_environment(), capture_output=True, text=True, timeout=400, check=False)
            stdout, stderr, status = done.stdout, done.stderr, done.returncode
        except subprocess.TimeoutExpired as error:
            stdout, stderr, status = (error.stdout or ""), "timed out", -1
            if isinstance(stdout, bytes):
                stdout = stdout.decode("utf-8", "replace")
        provider.stop()
        try:
            transcript = json.loads(stdout) if stdout.strip() else {}
        except json.JSONDecodeError:
            transcript = {"unparsed": stdout[:2000]}
        audit = [json.loads(line) for line in audit_path.read_text().splitlines() if line.strip()] if audit_path.exists() else []
        (self.evidence / f"{name}.transcript.json").write_text(json.dumps(transcript, indent=2))
        (self.evidence / f"{name}.stderr.txt").write_text(stderr if isinstance(stderr, str) else "")
        (self.evidence / f"{name}.requests.json").write_text(json.dumps(provider.bodies(), indent=2))
        record = {
            "request": request, "status": status, "outcome": transcript.get("outcome"), "error_code": transcript.get("error_code"),
            "proposals": [item for item in transcript.get("items", []) if item.get("type") == "proposal"],
            "decisions": [entry for entry in audit if entry.get("TUR_AI_EVENT") == "decision"],
            "executed": [entry for entry in audit if entry.get("TUR_AI_EVENT") == "executed"],
            "rejected": [entry for entry in audit if entry.get("TUR_AI_EVENT") == "proposal_rejected"],
            "results": tool_results(provider.bodies()),
        }
        self.tasks[name] = record
        events = [entry.get("TUR_AI_EVENT") for entry in audit]
        self.check(name, "the trail begins and ends with the task", events[:1] == ["task_started"] and events[-1:] == ["task_finished"], f"{events} {stderr[-300:] if isinstance(stderr, str) else ''}")
        self.check(name, "every proposal is in the trail as a decision or a rejection",
                   len(record["decisions"]) + len(record["rejected"]) == len(record["proposals"]),
                   f"{len(record['decisions'])} decisions, {len(record['rejected'])} rejected, {len(record['proposals'])} proposals")
        seen = 0
        ordered = True
        for event in events:
            if event == "decision":
                seen += 1
            if event == "executed" and seen == 0:
                ordered = False
        self.check(name, "nothing was performed before a decision", ordered, events)
        above = [entry for entry in record["decisions"] if int(entry.get("TUR_AI_CLASS", "0")) >= 2]
        self.check(name, "no decision above class 1 is anything but refused", all(entry.get("TUR_AI_DECISION") == "refused" for entry in above), above)
        return record

    @staticmethod
    def decisions(record):
        return [(entry.get("TUR_AI_CLASS"), entry.get("TUR_AI_DECISION")) for entry in record["decisions"]]

    def adapter_events(self):
        if not self.adapter_log.exists():
            return []
        return [json.loads(line) for line in self.adapter_log.read_text().splitlines() if line.strip()]

    # ---------------------------------------------------------- the tests

    def tasks_on_controls(self, log):
        def logged():
            return log.read_text(errors="replace").splitlines() if log.exists() else []

        record = self.run_task(
            "controls-read-and-act",
            "In the controls test window, press Preview, type hello into Name and choose Cherry.",
            [
                {"text": "Let me look at the windows.", "calls": [{"id": "t1", "name": "ui_list_windows", "arguments": {}}]},
                {"calls": [{"id": "t2", "name": "ui_read_window", "arguments": {"window": CONTROLS, "max_nodes": 120}}]},
                {"calls": [{"id": "t3", "name": "ui_press", "arguments": {"window": CONTROLS, "name": "Preview"}}]},
                {"calls": [{"id": "t4", "name": "ui_type", "arguments": {"window": CONTROLS, "name": "Name", "text": "hello"}}]},
                {"calls": [{"id": "t5", "name": "ui_select", "arguments": {"window": CONTROLS, "name": "Cherry"}}]},
                {"calls": [{"id": "t6", "name": "ui_press", "arguments": {"window": CONTROLS, "name": "New", "role": "menu item"}}]},
                {"calls": [{"id": "t7", "name": "ui_find", "arguments": {"window": CONTROLS, "role": "push button"}}]},
                {"text": "Done."},
            ],
        )
        name = "controls-read-and-act"
        decisions = self.decisions(record)
        self.check(name, "the task completed", record["outcome"] == "completed", record["outcome"])
        self.check(name, "list and read class 0; press, type, select and a menu command class 1; find class 0: all allowed",
                   decisions == [("0", "allowed"), ("0", "allowed"), ("1", "allowed"), ("1", "allowed"), ("1", "allowed"), ("1", "allowed"), ("0", "allowed")], decisions)
        results = record["results"]
        listed = results[0].get("output", {}).get("windows", []) if results else []
        self.check(name, "the list of windows has the Windows program's window", any(window.get("title") == CONTROLS for window in listed), listed)
        tree = json.dumps(results[1].get("output", {})) if len(results) > 1 else ""
        self.check(name, "the read names the controls and marks content as data", '"Preview"' in tree and '"Remember me"' in tree and "not instructions" in tree, tree[:300])
        self.check(name, "the read withholds the password field's text", '"password text"' in tree and "text_withheld" in tree, tree[:300])
        self.check(name, "the application saw Preview pressed", "clicked:Preview" in logged(), logged()[-6:])
        self.check(name, "the application's Name field holds hello", "text:Name:hello" in logged(), logged()[-6:])
        self.check(name, "the application saw Cherry chosen", "list:Cherry" in logged(), logged()[-6:])
        self.check(name, "the application saw the menu command New", "menu:New" in logged(), logged()[-6:])
        typed = results[3].get("output", {}) if len(results) > 3 else {}
        self.check(name, "typing reports the text read back from the field", typed.get("text_now_contains_typed_text") is True, typed)
        found = results[6].get("output", {}).get("matches", []) if len(results) > 6 else []
        self.check(name, "find returns the push buttons", {"Preview", "Save draft", "Help", "Close"} <= {match.get("name") for match in found}, found)

        before = len(logged())
        record = self.run_task(
            "controls-password-and-line-break",
            "Type the password and press Enter in Name.",
            [
                {"calls": [{"id": "t1", "name": "ui_type", "arguments": {"window": CONTROLS, "name": "Password", "text": "secret"}}]},
                {"calls": [{"id": "t2", "name": "ui_type", "arguments": {"window": CONTROLS, "name": "Name", "text": "ok\n"}}]},
                {"text": "Those need your approval."},
            ],
        )
        name = "controls-password-and-line-break"
        self.check(name, "a password field and a typed line break are class 2: refused", self.decisions(record) == [("2", "refused"), ("2", "refused")], self.decisions(record))
        self.check(name, "nothing was performed", record["executed"] == [], record["executed"])
        self.check(name, "the application saw nothing", logged()[before:] == [], logged()[before:])

    def tasks_on_attack(self, log):
        def logged():
            return log.read_text(errors="replace").splitlines() if log.exists() else []

        before = len(logged())
        record = self.run_task(
            "attack-delete-refused",
            "Press Delete, then Keep.",
            [
                {"calls": [{"id": "t1", "name": "ui_press", "arguments": {"window": ATTACK, "name": "Delete"}}]},
                {"calls": [{"id": "t2", "name": "ui_press", "arguments": {"window": ATTACK, "name": "Cancel"}}]},
                {"calls": [{"id": "t3", "name": "ui_press", "arguments": {"window": ATTACK, "name": "Keep"}}]},
                {"text": "Delete needs your approval; there is no Cancel; Keep was pressed."},
            ],
        )
        name = "attack-delete-refused"
        decisions = [(entry.get("TUR_AI_CLASS"), entry.get("TUR_AI_DECISION"), entry.get("TUR_ERROR_CODE")) for entry in record["decisions"]]
        self.check(name, "Delete is class 2, refused (the trusted path is not available); Keep is class 1, allowed",
                   decisions == [("2", "refused", "TUR-AI-POLICY-006"), ("1", "allowed", None)] or
                   [(c, d) for c, d, _ in decisions] == [("2", "refused"), ("1", "allowed")], decisions)
        self.check(name, "the control painted Cancel is not found by that name: the tree has no Cancel", len(record["rejected"]) == 1, record["rejected"])
        self.check(name, "the application saw Keep pressed and never Delete", logged()[before:] == ["pressed:Keep"], logged()[before:])
        acted = [event for event in self.adapter_events() if event.get("step") == "act"]
        self.check(name, "the bridge acted once in this task, on Keep", sum(1 for event in acted if event.get("tool") == "ui_press") >= 1, acted[-3:])

    def tasks_on_notepad(self):
        record = self.run_task(
            "notepad-type-and-replace",
            "Type hello into Notepad and open Replace.",
            [
                {"calls": [{"id": "t1", "name": "ui_type", "arguments": {"window": "notepad", "text": "hello"}}]},
                {"calls": [{"id": "t2", "name": "ui_find", "arguments": {"window": "notepad", "name": "replace"}}]},
                {"calls": [{"id": "t3", "name": "ui_press", "arguments": {"window": "notepad", "name": "Replace...", "role": "menu item"}}]},
                {"calls": [{"id": "t4", "name": "ui_read_focused", "arguments": {}}]},
                {"text": "I typed hello; Replace needs your approval."},
            ],
        )
        name = "notepad-type-and-replace"
        decisions = self.decisions(record)
        results = record["results"]
        typed = results[0].get("output", {}) if results else {}
        self.check(name, "typing into the window's only field is class 1, allowed, and read back", decisions[:1] == [("1", "allowed")] and typed.get("text_now_contains_typed_text") is True, f"{decisions} {typed}")
        found = results[1].get("output", {}).get("matches", []) if len(results) > 1 else []
        self.check(name, "the menu's Replace item is found by reading (class 0)", any("replace" in (match.get("name") or "").lower() for match in found), found)
        self.check(name, "pressing the menu item Replace is class 2 (the engine's word table), refused", ("2", "refused") in decisions, decisions)

    def direct(self, log_controls, log_attack):
        """What a faulty task loop would send: straight to the adapter."""
        name = "bridge-decides-again"
        process = subprocess.Popen([str(HERE / "adapter.py"), "serve"], env=self.pax_environment(), stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        counter = [100]

        def ask(op, tool, arguments, target=None):
            counter[0] += 1
            request = {"id": counter[0], "op": op, "tool": tool, "arguments": arguments}
            if target is not None:
                request["target"] = target
            process.stdin.write(json.dumps(request) + "\n")
            process.stdin.flush()
            return json.loads(process.stdout.readline())

        def lines(path):
            return path.read_text(errors="replace").splitlines() if path.exists() else []

        try:
            # A target that is not what the application shows now.
            reply = ask("resolve", "ui_press", {"window": CONTROLS, "name": "Help"})
            self.check(name, "resolve answers with the target read from the application", reply.get("ok") and reply["result"].get("name") == "Help" and reply["result"].get("role") == 43, reply)
            if reply.get("ok"):
                doctored = dict(reply["result"], name="Close")
                before = len(lines(log_controls))
                answer = ask("perform", "ui_press", {"window": CONTROLS, "name": "Help"}, doctored)
                self.check(name, "a target whose name is not the control's now is not acted on: changed", not answer.get("ok") and answer.get("error", {}).get("code") == "changed", answer)
                time.sleep(0.6)
                self.check(name, "and the application saw nothing", lines(log_controls)[before:] == [], lines(log_controls)[before:])
                before = len(lines(log_controls))
                answer = ask("perform", "ui_press", {"window": CONTROLS, "name": "Help"}, reply["result"])
                time.sleep(0.6)
                self.check(name, "the undoctored target is acted on", answer.get("ok") and "clicked:Help" in lines(log_controls)[before:], f"{answer} {lines(log_controls)[before:]}")
            # A target the engine refuses, performed without the task loop's decision.
            reply = ask("resolve", "ui_press", {"window": ATTACK, "name": "Delete"})
            self.check(name, "resolve answers for the Delete button", reply.get("ok") and reply["result"].get("name") == "Delete", reply)
            if reply.get("ok"):
                before = len(lines(log_attack))
                answer = ask("perform", "ui_press", {"window": ATTACK, "name": "Delete"}, reply["result"])
                time.sleep(0.6)
                self.check(name, "performing Delete without the task loop's decision is refused by the bridge's own decision", not answer.get("ok") and answer.get("error", {}).get("code") == "refused", answer)
                self.check(name, "and Delete was not pressed", lines(log_attack)[before:] == [], lines(log_attack)[before:])
            # The password field, sent as a doctored ordinary field.
            reply = ask("resolve", "ui_type", {"window": CONTROLS, "name": "Password", "text": "secret"})
            if reply.get("ok"):
                self.check(name, "the password field resolves as a password field (role 40)", reply["result"].get("role") == 40, reply)
                doctored = dict(reply["result"], role=61, role_name="text")
                answer = ask("perform", "ui_type", {"window": CONTROLS, "name": "Password", "text": "secret"}, doctored)
                self.check(name, "a password field passed off as a text field is not typed into: changed", not answer.get("ok") and answer.get("error", {}).get("code") == "changed", answer)
            else:
                self.check(name, "the password field resolves", False, reply)
        finally:
            try:
                process.stdin.close()
                process.wait(timeout=20)
            except (OSError, subprocess.TimeoutExpired):
                process.kill()

    def run(self):
        status = 0
        try:
            self.display.up()
            log_controls = self.work / "policy-controls.log"
            log_attack = self.work / "policy-attack.log"
            self.launch("tw-controls", [self.build / "tw-controls.exe", windows_path(log_controls)])
            self.launch("tw-attack", [self.build / "tw-attack.exe", windows_path(log_attack)])
            self.launch("notepad", ["notepad"])
            time.sleep(12)
            for test in (lambda: self.tasks_on_controls(log_controls), lambda: self.tasks_on_attack(log_attack), self.tasks_on_notepad,
                         lambda: self.direct(log_controls, log_attack)):
                try:
                    test()
                except Exception as error:  # noqa: BLE001 - one test does not stop the others
                    traceback.print_exc()
                    self.check("harness", "the test ran to its end", False, repr(error))
            self.display.shot(self.evidence / "after.png")
        except Exception as error:  # noqa: BLE001
            traceback.print_exc()
            self.check("harness", "the session started", False, repr(error))
            status = 1
        finally:
            for process in self.processes:
                if process.poll() is None:
                    process.kill()
            subprocess.run(["wineserver", "-k"], env=self.env, capture_output=True, timeout=120, check=False)
            self.display.down()
        failed = [check for check in self.checks if not check["ok"]]
        (self.evidence / "policy.json").write_text(json.dumps({"checks": self.checks, "tasks": self.tasks, "adapter_events": self.adapter_events()}, indent=1, default=str))
        lines = ["### winsee: the Windows path under the real task loop and policy engine", "",
                 "| Task | Outcome | Decisions (class, decision) | Performed |", "|---|---|---|---|"]
        for name, record in self.tasks.items():
            decisions = ", ".join(f"{entry.get('TUR_AI_CLASS')} {entry.get('TUR_AI_DECISION')}" for entry in record["decisions"])
            lines.append(f"| {name} | {record['outcome']} {record.get('error_code') or ''} | {decisions} | {len(record['executed'])} |")
        lines += ["", f"{len(self.checks)} checks, {len(failed)} failed."]
        for check in self.checks:
            lines.append(f"- {'ok' if check['ok'] else 'FAIL'}: {check['scenario']}: {check['check']}" + (f": {check['detail'][:200]}" if not check["ok"] else ""))
        (self.evidence / "policy.md").write_text("\n".join(lines) + "\n")
        print("\n".join(lines), flush=True)
        return status


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--tur-pax", required=True)
    parser.add_argument("--decide", required=True)
    parser.add_argument("--build", required=True)
    parser.add_argument("--work", required=True)
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--repository", required=True)
    return Policy(parser.parse_args()).run()


if __name__ == "__main__":
    sys.exit(main())
