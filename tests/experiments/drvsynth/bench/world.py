# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Client for the emulated device's "physical world" socket: room
temperature, button presses, and read-back of what the device shows and
stores. Used by the capture sessions and the scoring suite; speaks only in
user-level terms (degrees, button numbers, pictures, note texts)."""

from __future__ import annotations

import json
import socket


class World:
    def __init__(self, path: str) -> None:
        self.path = path

    def call(self, **request) -> dict:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
            s.settimeout(10)
            s.connect(self.path)
            s.sendall((json.dumps(request) + "\n").encode())
            data = b""
            while not data.endswith(b"\n"):
                chunk = s.recv(65536)
                if not chunk:
                    break
                data += chunk
        reply = json.loads(data)
        if not reply.get("ok"):
            raise RuntimeError(f"world: {request} -> {reply}")
        return reply

    def set(self, **values) -> None:
        self.call(op="set", **values)

    def press(self, button: int, seconds: float) -> bool:
        return self.call(op="press", button=button, seconds=seconds).get("delivered", False)

    def note(self, slot: int, text: str) -> None:
        self.call(op="note", slot=slot, text=text)

    def reset(self) -> None:
        self.call(op="reset")

    def state(self) -> dict:
        return self.call(op="state")["state"]
