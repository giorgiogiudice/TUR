#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The Linux side of tur-winbridge.exe: starts it under Wine in a prefix
and exchanges one JSON object a line with it over its standard input and
output (tests/experiments/winsee/bridge/tw_core.c says what it answers).

Used by the experiment's harness (winsee.py), by the adapter that stands
between the P@x task loop and the bridge (adapter.py), and by nothing else.
"""

import json
import os
import queue
import subprocess
import threading
import time


class BridgeError(Exception):
    """The bridge did not answer, or answered with an error."""


class Bridge:
    """One tur-winbridge.exe process in a Wine prefix."""

    def __init__(self, program, environment, log_path, wine="wine", debug="-all"):
        self.program = str(program)
        self.environment = dict(environment)
        self.environment["WINEDEBUG"] = debug
        self.log_path = str(log_path)
        self.wine = wine
        self.process = None
        self.lines = None
        self.next_id = 1
        self.starts = 0
        self.failures = []
        self.started_seconds = None

    def start(self):
        log = open(self.log_path, "ab")
        began = time.monotonic()
        self.process = subprocess.Popen(
            [self.wine, self.program],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=log,
            env=self.environment,
        )
        log.close()
        self.lines = queue.Queue()
        self.starts += 1

        def read(stream, lines):
            for raw in stream:
                lines.put(raw)
            lines.put(None)

        threading.Thread(target=read, args=(self.process.stdout, self.lines), daemon=True).start()
        hello = self._exchange({"op": "hello"}, 120)
        self.started_seconds = time.monotonic() - began
        return hello

    def running(self):
        return self.process is not None and self.process.poll() is None

    def stop(self):
        process, self.process = self.process, None
        if process is None:
            return
        try:
            if process.poll() is None:
                process.stdin.write(b'{"id":0,"op":"quit"}\n')
                process.stdin.flush()
                process.wait(timeout=5)
        except (OSError, subprocess.TimeoutExpired):
            pass
        if process.poll() is None:
            process.kill()
            process.wait()

    def _exchange(self, request, timeout):
        request = dict(request)
        request["id"] = self.next_id
        self.next_id += 1
        try:
            self.process.stdin.write(json.dumps(request).encode("ascii") + b"\n")
            self.process.stdin.flush()
        except OSError as error:
            raise BridgeError(f"the bridge's input is closed: {error}") from error
        deadline = time.monotonic() + timeout
        while True:
            left = deadline - time.monotonic()
            if left <= 0:
                raise BridgeError(f"no answer to {request.get('op')} in {timeout} s")
            try:
                raw = self.lines.get(timeout=left)
            except queue.Empty:
                raise BridgeError(f"no answer to {request.get('op')} in {timeout} s") from None
            if raw is None:
                raise BridgeError(f"the bridge ended while answering {request.get('op')} (status {self.process.poll()})")
            text = raw.decode("utf-8", "replace").strip()
            if not text.startswith("{"):
                continue  # something Wine wrote on standard output
            try:
                reply = json.loads(text)
            except json.JSONDecodeError as error:
                raise BridgeError(f"not JSON ({error}): {text[:300]}") from error
            if reply.get("id") != request["id"]:
                continue
            if not reply.get("ok"):
                raise BridgeError(json.dumps(reply.get("error")))
            return reply.get("result", {})

    def ask(self, op, timeout=40, **fields):
        """One request. A bridge that died or does not answer is replaced by
        a new one for the next request; this one raises BridgeError."""
        if not self.running():
            self.start()
        request = {"op": op}
        request.update({key: value for key, value in fields.items() if value is not None})
        try:
            return self._exchange(request, timeout)
        except BridgeError as error:
            self.failures.append({"op": op, "fields": {k: str(v)[:80] for k, v in fields.items()}, "error": str(error)[:400]})
            if "no answer" in str(error) or "ended" in str(error) or "closed" in str(error):
                self.kill()
            raise

    def kill(self):
        process, self.process = self.process, None
        if process is not None and process.poll() is None:
            process.kill()
            process.wait()

    def memory(self):
        """Resident and proportional memory of the bridge's process, in kB."""
        if not self.running():
            return {}
        figures = {}
        try:
            with open(f"/proc/{self.process.pid}/smaps_rollup", encoding="ascii") as rollup:
                for line in rollup:
                    parts = line.split()
                    if len(parts) >= 2 and parts[0] in ("Rss:", "Pss:", "Private_Dirty:", "Private_Clean:"):
                        figures[parts[0].rstrip(":").lower() + "_kb"] = int(parts[1])
            with open(f"/proc/{self.process.pid}/comm", encoding="utf-8") as comm:
                figures["comm"] = comm.read().strip()
        except OSError as error:
            figures["error"] = str(error)
        return figures


def windows_path(path):
    """A path of this machine as the prefix sees it (drive Z: is /)."""
    return "Z:" + str(path).replace("/", "\\")


def wine_environment(prefix, display=":99", extra=None):
    environment = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": os.environ.get("HOME", "/tmp"),
        "LANG": "C.UTF-8",
        "LC_ALL": "C.UTF-8",
        "DISPLAY": display,
        "WINEPREFIX": str(prefix),
        "WINEDEBUG": "-all",
        "WINEDLLOVERRIDES": "mscoree,mshtml=",
    }
    for name in ("XDG_RUNTIME_DIR", "USER", "LOGNAME"):
        if name in os.environ:
            environment[name] = os.environ[name]
    if extra:
        environment.update(extra)
    return environment
