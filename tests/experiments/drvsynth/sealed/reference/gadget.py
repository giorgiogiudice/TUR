#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""SEALED: the reference Linux driver of the invented desk device, written
from sealed/PROTOCOL.md by the bench's author (not blind). It follows the
command-line contract of handoff/TASK.md and proves that the scoring suite
can be passed. A user-space driver: pyusb over libusb over usbfs.

  gadget.py COMMAND [ARGS]
"""

from __future__ import annotations

import struct
import sys
import time

import usb.core
import usb.util

VID, PID = 0x1209, 0x000C
EP_OUT, EP_IN, EP_EVENT = 0x02, 0x81, 0x83
KEY = 0x6D2B79F5
TIMEOUT_MS = 2000
MODE_DELAY = 0.25


class DeviceError(Exception):
    pass


def checksum(data: bytes) -> bytes:
    s1, s2 = 0x5A, 0
    for b in data:
        s1 = (s1 + b) % 251
        s2 = (s2 + s1) % 251
    return bytes([s2, s1])


def auth_response(nonce: int) -> int:
    return (((nonce << 5) | (nonce >> 27)) & 0xFFFFFFFF) ^ KEY


def decode_temperature(two: bytes) -> int:
    return struct.unpack(">h", two)[0]


class Gadget:
    def __init__(self) -> None:
        self.dev = usb.core.find(idVendor=VID, idProduct=PID)
        if self.dev is None:
            print("error: no device", file=sys.stderr)
            sys.exit(3)
        usb.util.claim_interface(self.dev, 0)
        self.seq = 0

    # -- session
    def connect(self) -> None:
        hello = bytes(self.dev.ctrl_transfer(0xC0, 0x01, 0, 0, 8, timeout=TIMEOUT_MS))
        nonce = struct.unpack_from("<I", hello, 1)[0]
        r = auth_response(nonce)
        self.dev.ctrl_transfer(0x40, 0x02, r >> 16, r & 0xFFFF, None, timeout=TIMEOUT_MS)
        self.seq = 0

    def bye(self) -> None:
        try:
            self.dev.ctrl_transfer(0x40, 0x03, 0, 0, None, timeout=TIMEOUT_MS)
        finally:
            usb.util.release_interface(self.dev, 0)

    # -- frames
    def read_frame(self, seq: int) -> tuple[int, bytes]:
        raw = bytes(self.dev.read(EP_IN, 512, timeout=TIMEOUT_MS))
        if len(raw) < 6 or raw[0] != 0x9D or raw[1] != len(raw) - 4 or raw[2] != seq:
            raise DeviceError(f"malformed response {raw.hex()}")
        if checksum(raw[1:-2]) != raw[-2:]:
            raise DeviceError("response checksum")
        return raw[3], raw[4:-2]

    def mode_delay(self) -> None:
        time.sleep(MODE_DELAY)

    def transact(self, cmd: int, payload: bytes = b"", accept: tuple = ()) -> tuple[int, bytes]:
        self.seq = self.seq % 255 + 1
        body = bytes([len(payload) + 2, self.seq, cmd]) + payload
        self.dev.write(EP_OUT, bytes([0x9C]) + body + checksum(body), timeout=TIMEOUT_MS)
        status, p = self.read_frame(self.seq)
        if status == 0x02:
            data = b""
            for i in range(p[1]):
                st, q = self.read_frame(self.seq)
                if st != 0x03 or q[0] != i:
                    raise DeviceError("multi-frame response out of order")
                data += q[1:]
            status, p = 0x00, data[:p[0]]
        if cmd == 0x10 and status == 0x00:
            self.mode_delay()
        if status != 0x00 and status not in accept:
            raise DeviceError(f"device refused command {cmd:#04x}: status {status:#04x}")
        return status, p

    # -- helpers
    def get_mode(self) -> int:
        return self.transact(0x11)[1][0]

    def set_mode(self, mode: int) -> None:
        self.transact(0x10, bytes([mode]))

    def in_mode(self, mode: int, cmd: int, payload: bytes = b"") -> bytes:
        was = self.get_mode()
        if was != mode:
            self.set_mode(mode)
        p = self.transact(cmd, payload)[1]
        if was != mode:
            self.set_mode(was)
        return p

    def sensors(self) -> tuple[int, int, int]:
        p = self.transact(0x30)[1]
        return decode_temperature(p[0:2]), struct.unpack_from("<H", p, 2)[0], struct.unpack_from("<I", p, 4)[0]

    def clear_notes(self) -> None:
        if self.transact(0x56, accept=(0x01,))[0] == 0x01:
            self.transact(0x56)

    def event_line(self, report: bytes) -> str | None:
        if report[0] == 0x01:
            return f"button {report[2]} {struct.unpack_from('<H', report, 3)[0] / 100:.2f}"
        if report[0] == 0x02:
            return f"alert {decode_temperature(report[2:4]) / 100:.2f}"
        return None  # heartbeats


def parse_int(text: str, lo: int, hi: int) -> int:
    try:
        v = int(text)
    except ValueError:
        v = None
    if v is None or not lo <= v <= hi:
        print(f"error: '{text}' is not a number from {lo} to {hi}", file=sys.stderr)
        sys.exit(2)
    return v


def read_image(path: str) -> bytes:
    rows = [line.rstrip("\r\n") for line in open(path, encoding="ascii")]
    rows = [r for r in rows if r]
    if len(rows) != 8 or any(len(r) != 16 for r in rows):
        print("error: an image is 8 rows of 16 pixels", file=sys.stderr)
        sys.exit(2)
    pix = [[0 if ch == "." else int(ch, 16) for ch in r] for r in rows]
    return bytes((pix[2 * k][c] << 4) | pix[2 * k + 1][c] for c in range(16) for k in range(4))


def run(g: Gadget, argv: list[str]) -> int:
    cmd, args = argv[0], argv[1:]
    if cmd == "info":
        p = g.transact(0x01)[1]
        print(f"firmware {p[0]}.{p[1]}.{p[2]}")
        print(f"uptime {struct.unpack_from('<I', p, 6)[0]}")
    elif cmd in ("read-temperature", "read-humidity", "read-light", "read-sensors"):
        t, h, lux = g.sensors()
        if cmd == "read-temperature":
            print(f"{t / 100:.2f}")
        elif cmd == "read-humidity":
            print(f"{h / 10:.1f}")
        elif cmd == "read-light":
            print(lux)
        else:
            print(f"temperature {t / 100:.2f}\nhumidity {h / 10:.1f}\nlight {lux}")
    elif cmd == "get-mode":
        print({0: "day", 1: "night", 2: "setup"}.get(g.get_mode(), "unknown"))
    elif cmd == "set-mode" and len(args) == 1 and args[0] in ("day", "night"):
        g.set_mode(1 if args[0] == "night" else 0)
        print(f"mode {args[0]}")
    elif cmd in ("set-brightness", "set-night-level") and len(args) == 1:
        v = parse_int(args[0], 0, 100)
        night = cmd == "set-night-level"
        g.in_mode(1 if night else 0, 0x21, bytes([v]))
        print(f"{'night-level' if night else 'brightness'} {v}")
    elif cmd in ("get-brightness", "get-night-level"):
        print(g.in_mode(1 if cmd == "get-night-level" else 0, 0x22)[0])
    elif cmd == "set-alert" and len(args) == 1:
        if args[0] == "off":
            v = 0x7FFF
        else:
            try:
                t = float(args[0])
            except ValueError:
                t = None
            if t is None or not -40.0 <= t <= 125.0:
                print("error: an alert is -40.0 to 125.0 or off", file=sys.stderr)
                return 2
            v = int((t * 10) + (0.5 if t >= 0 else -0.5))
        g.in_mode(2, 0x21, struct.pack("<h", v))
        print("alert off" if v == 0x7FFF else f"alert {v / 10:.1f}")
    elif cmd == "get-alert":
        v = struct.unpack("<h", g.in_mode(2, 0x22)[:2])[0]
        print("off" if v == 0x7FFF else f"{v / 10:.1f}")
    elif cmd == "show-image" and len(args) == 1:
        packed = read_image(args[0])
        g.transact(0x40, bytes([0]) + packed[:32])
        g.transact(0x40, bytes([32]) + packed[32:])
        g.transact(0x41)
        print("image shown")
    elif cmd == "clear-display":
        g.transact(0x42)
        print("display cleared")
    elif cmd == "list-notes":
        p = g.transact(0x50)[1]
        for slot in range(8):
            if p[0] & (1 << slot):
                print(f"{slot} {p[1 + slot]}")
    elif cmd == "write-note" and len(args) == 2:
        slot, text = parse_int(args[0], 0, 7), args[1].encode()
        if not 1 <= len(text) <= 120:
            print("error: a note has 1 to 120 bytes", file=sys.stderr)
            return 2
        g.transact(0x51, bytes([slot, len(text)]))
        for i in range(0, len(text), 40):
            g.transact(0x52, bytes([i // 40]) + text[i:i + 40])
        g.transact(0x53)
        print(f"note {slot} stored")
    elif cmd == "read-note" and len(args) == 1:
        slot = parse_int(args[0], 0, 7)
        status, p = g.transact(0x54, bytes([slot]), accept=(0x86,))
        if status == 0x86:
            print("empty")
            return 4
        print(p.decode("utf-8", "replace"))
    elif cmd == "delete-note" and len(args) == 1:
        slot = parse_int(args[0], 0, 7)
        g.transact(0x55, bytes([slot]))
        print(f"note {slot} deleted")
    elif cmd == "clear-notes":
        g.clear_notes()
        print("notes cleared")
    elif cmd == "wait-event" and len(args) == 1:
        end = time.monotonic() + parse_int(args[0], 1, 600)
        while time.monotonic() < end:
            wait = int(min(500, max(1, (end - time.monotonic()) * 1000)))
            try:
                report = bytes(g.dev.read(EP_EVENT, 8, timeout=wait))
            except usb.core.USBError as exc:
                if getattr(exc, "errno", None) == 110 or "timeout" in str(exc).lower():
                    continue
                raise
            line = g.event_line(report)
            if line:
                print(line, flush=True)
    else:
        print("usage: gadget.py COMMAND [ARGS] (see handoff/TASK.md)", file=sys.stderr)
        return 2
    return 0


def main(argv: list[str], gadget_class=Gadget) -> int:
    if not argv:
        print("usage: gadget.py COMMAND [ARGS]", file=sys.stderr)
        return 2
    g = gadget_class()
    try:
        g.connect()
        rc = run(g, argv)
    except (DeviceError, usb.core.USBError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        rc = 1
    try:
        g.bye()
    except usb.core.USBError:
        pass
    return rc


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
