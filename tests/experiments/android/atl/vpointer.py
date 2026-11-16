#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""A virtual pointer for a headless Wayland compositor, held open for the whole session.

    vpointer.py COMMAND_FILE OUTPUT_WIDTH OUTPUT_HEIGHT

A headless compositor has no input devices, so its seat advertises no
pointer and applications never ask for pointer events: a click injected for
an instant arrives before the application is listening. This program
creates one virtual pointer (wlr-virtual-pointer-unstable-v1) and keeps it,
so the seat has a pointer from the start, and it performs the clicks the
harness asks for by appending lines to COMMAND_FILE:

    click X Y        move to (X, Y) in output pixels, press and release the left button
    move X Y         move only

It speaks the Wayland wire protocol directly, so the test container needs
no bindings. Test equipment only.
"""

import os
import select
import socket
import struct
import sys
import time

BTN_LEFT = 0x110


def string(text: str) -> bytes:
    raw = text.encode() + b"\0"
    return struct.pack("<I", len(raw)) + raw + b"\0" * (-len(raw) % 4)


def main() -> int:
    command_file, width, height = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    path = os.path.join(os.environ["XDG_RUNTIME_DIR"], os.environ.get("WAYLAND_DISPLAY", "wayland-0"))
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(path)

    def send(obj: int, opcode: int, payload: bytes = b"") -> None:
        sock.sendall(struct.pack("<IHH", obj, opcode, 8 + len(payload)) + payload)

    globals_: dict[str, tuple[int, int]] = {}
    buf = b""

    def pump(until_callback: int | None, timeout: float) -> bool:
        """Read events; True when the callback with this id fired."""
        nonlocal buf
        deadline = time.monotonic() + timeout
        while True:
            while len(buf) >= 8:
                obj, opcode, size = struct.unpack_from("<IHH", buf)
                if size < 8 or len(buf) < size:
                    break
                body, buf = buf[8:size], buf[size:]
                if obj == 2 and opcode == 0:  # wl_registry.global
                    name, slen = struct.unpack_from("<II", body)
                    iface = body[8:8 + slen - 1].decode()
                    version = struct.unpack_from("<I", body, 8 + ((slen + 3) & ~3))[0]
                    globals_[iface] = (name, version)
                elif obj == 1 and opcode == 0:  # wl_display.error
                    target, code, slen = struct.unpack_from("<III", body)
                    print(f"wayland error on object {target}: code {code}: {body[12:12 + slen - 1].decode(errors='replace')}", flush=True)
                    sys.exit(1)
                elif until_callback is not None and obj == until_callback and opcode == 0:
                    return True
            left = deadline - time.monotonic()
            if left <= 0:
                return False
            ready, _, _ = select.select([sock], [], [], left)
            if ready:
                data = sock.recv(65536)
                if not data:
                    print("the compositor closed the connection", flush=True)
                    sys.exit(0)
                buf += data

    send(1, 1, struct.pack("<I", 2))  # wl_display.get_registry -> object 2
    send(1, 0, struct.pack("<I", 3))  # wl_display.sync -> callback 3
    if not pump(3, 10):
        print("no answer from the compositor", flush=True)
        return 1
    if "zwlr_virtual_pointer_manager_v1" not in globals_ or "wl_seat" not in globals_:
        print(f"the compositor offers no virtual pointer: {sorted(globals_)}", flush=True)
        return 1
    seat, manager, pointer = 4, 5, 6
    name, version = globals_["wl_seat"]
    send(2, 0, struct.pack("<I", name) + string("wl_seat") + struct.pack("<II", min(version, 5), seat))
    name, version = globals_["zwlr_virtual_pointer_manager_v1"]
    send(2, 0, struct.pack("<I", name) + string("zwlr_virtual_pointer_manager_v1") + struct.pack("<II", 1, manager))
    send(manager, 0, struct.pack("<II", seat, pointer))  # create_virtual_pointer(seat, id)
    send(1, 0, struct.pack("<I", 7))  # sync, to learn of a protocol error at once
    pump(7, 10)
    print(f"virtual pointer created on {path}", flush=True)

    def now() -> int:
        return int(time.monotonic() * 1000) & 0xFFFFFFFF

    def move(x: int, y: int) -> None:
        send(pointer, 1, struct.pack("<IIIII", now(), x, y, width, height))  # motion_absolute
        send(pointer, 4)  # frame

    def button(state: int) -> None:
        send(pointer, 2, struct.pack("<III", now(), BTN_LEFT, state))
        send(pointer, 4)

    open(command_file, "a").close()
    offset = 0
    while True:
        pump(None, 0.05)
        try:
            with open(command_file) as f:
                f.seek(offset)
                lines = f.readlines()
                offset = f.tell()
        except OSError:
            lines = []
        for line in lines:
            parts = line.split()
            if len(parts) == 3 and parts[0] in ("click", "move"):
                x, y = int(parts[1]), int(parts[2])
                move(x, y)
                pump(None, 0.15)
                if parts[0] == "click":
                    button(1)
                    pump(None, 0.08)
                    button(0)
                print(f"{parts[0]} {x} {y}", flush=True)


if __name__ == "__main__":
    sys.exit(main())
