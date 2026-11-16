# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""A small QMP client: one command at a time, events kept aside."""

from __future__ import annotations

import json
import socket
import time


class QmpError(Exception):
    pass


class Qmp:
    def __init__(self, path: str, timeout: float = 30.0):
        deadline = time.time() + timeout
        last: Exception | None = None
        while True:
            try:
                self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                self.sock.connect(path)
                break
            except OSError as error:
                last = error
                self.sock.close()
                if time.time() > deadline:
                    raise QmpError(f"cannot connect to {path}: {last}")
                time.sleep(0.2)
        self.sock.settimeout(timeout)
        self.buffer = b""
        self.events: list[dict] = []
        self._read()                       # the greeting
        self.command("qmp_capabilities")

    def _read(self) -> dict:
        while b"\n" not in self.buffer:
            chunk = self.sock.recv(65536)
            if not chunk:
                raise QmpError("QEMU closed the control socket")
            self.buffer += chunk
        line, self.buffer = self.buffer.split(b"\n", 1)
        return json.loads(line)

    def command(self, name: str, timeout: float | None = None, **arguments) -> dict:
        message: dict = {"execute": name}
        if arguments:
            message["arguments"] = arguments
        if timeout is not None:
            self.sock.settimeout(timeout)
        try:
            self.sock.sendall(json.dumps(message).encode() + b"\n")
            while True:
                reply = self._read()
                if "event" in reply:
                    self.events.append(reply)
                    continue
                if "error" in reply:
                    raise QmpError(f"{name}: {reply['error'].get('desc', reply['error'])}")
                return reply.get("return", {})
        except (OSError, ValueError) as error:
            raise QmpError(f"{name}: {error}")

    def hmp(self, line: str) -> str:
        return str(self.command("human-monitor-command", **{"command-line": line}))

    def close(self) -> None:
        try:
            self.sock.close()
        except OSError:
            pass
