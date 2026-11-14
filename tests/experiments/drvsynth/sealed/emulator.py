#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""SEALED: the invented device of the driver-synthesis experiment, emulated
in user space and plugged into the kernel's USB/IP virtual host controller
(vhci_hcd), with the attach and USB/IP code of the windows-drivers
experiment (tests/windows-drivers/virtual-usb/usbip_devices.py).

The protocol is specified in sealed/PROTOCOL.md. Blind engineers must not
read this file.

  emulator.py run --log-dir DIR --control SOCKET [--firmware 3.1.4]
                  [--serial QF2-...] [--uptime-offset SECONDS]

writes DIR/ready (JSON: bus, dev, node, sysfs) once the host has enumerated
the device, and serves until SIGTERM, then detaches its vhci port.

SOCKET is a Unix socket for the "physical world" and the scoring suite, one
JSON object per line, answered with one JSON line:

  {"op": "state"}                                  the device's state
  {"op": "set", "temperature": 21.37, "humidity": 45.2, "light": 312}
  {"op": "set", "day_brightness": 30, "night_level": 5, "mode": "night",
                "alert": 26.5 | null}              plant device state
  {"op": "note", "slot": 2, "text": "..."}         plant a note
  {"op": "reset"}                                  factory settings, no notes
  {"op": "press", "button": 1, "seconds": 0.42}    a button press

The socket's directory must be root's: whoever can reach it can read the
device's state.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import pathlib
import random
import signal
import socket
import struct
import sys
import threading
import time

HERE = pathlib.Path(__file__).resolve().parent
_USBIP = HERE.parents[2] / "windows-drivers" / "virtual-usb" / "usbip_devices.py"
_spec = importlib.util.spec_from_file_location("usbip_devices", _USBIP)
usbip = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(usbip)

VID, PID = 0x1209, 0x000C
EPIPE = usbip.EPIPE
KEY = 0x6D2B79F5
ALERT_OFF = 0x7FFF
MAGIC_CMD, MAGIC_RSP = 0x9C, 0x9D
MAX_PAYLOAD = 48
BUSY_SECONDS = 0.200
ARM_SECONDS = 2.0
NOTE_MAX, NOTE_CHUNK, SLOTS = 120, 40, 8
MODES = {0: "day", 1: "night", 2: "setup"}

OK, ARMED, MORE, CHUNK = 0x00, 0x01, 0x02, 0x03
BAD_CHECKSUM, NO_SESSION, BAD_LENGTH, BAD_ARG, UNKNOWN, BAD_STATE, EMPTY = 0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86


def checksum(data: bytes) -> bytes:
    s1, s2 = 0x5A, 0
    for b in data:
        s1 = (s1 + b) % 251
        s2 = (s2 + s1) % 251
    return bytes([s2, s1])


def rotl32(value: int, n: int) -> int:
    return ((value << n) | (value >> (32 - n))) & 0xFFFFFFFF


def auth_response(nonce: int) -> int:
    return rotl32(nonce, 5) ^ KEY


def frame(magic: int, seq: int, code: int, payload: bytes) -> bytes:
    body = bytes([len(payload) + 2, seq, code]) + payload
    return bytes([magic]) + body + checksum(body)


class Desk(usbip.Device):
    vid = VID

    def __init__(self, log: "usbip.Log", firmware: tuple[int, int, int], serial: str, uptime_offset: int) -> None:
        super().__init__(PID, "Glimmerdesk G2", log)
        self.lock = threading.RLock()
        self.firmware, self.serial = firmware, serial
        self.started = time.monotonic() - uptime_offset
        self.rng = random.SystemRandom()
        # The world.
        self.temperature, self.humidity, self.light = 2150, 400, 250  # 0.01 degC, 0.1 %, lux
        # Device state.
        self.mode = 0
        self.day_brightness, self.night_level, self.alert = 60, 10, ALERT_OFF
        self.buffer, self.shown = bytearray(64), bytearray(64)
        self.notes: dict[int, bytes] = {}
        self.writing: dict | None = None
        # Session state.
        self.nonce: int | None = None
        self.session = False
        self.last_seq: int | None = None
        self.last_response: list[bytes] = []
        self.armed_at: float | None = None
        self.busy_until = 0.0
        self.responses: list[bytes] = []
        self.events: list[bytes] = []
        self.event_counter = 0
        self.last_heartbeat = 0.0
        self.counters = {k: 0 for k in (
            "hello", "auth_ok", "auth_fail", "bye", "frames", "executed", "duplicates", "bad_checksum",
            "bad_frame", "no_session", "dropped_busy", "events_sent", "heartbeats_sent", "events_dropped",
            "shows", "clears_done", "clears_armed")}
        self.command_log: list[str] = []

    # ---------------------------------------------------------------- descriptors
    def uptime(self) -> int:
        return int(time.monotonic() - self.started)

    def descriptors(self) -> bytes:
        interface = bytes([9, 4, 0, 0, 3, 0xFF, 0x51, 0x02, 0])
        return usbip.config_desc(interface + usbip.endpoint(0x02, 2, 512, 0) + usbip.endpoint(0x81, 2, 512, 0) +
                                 usbip.endpoint(0x83, 3, 8, 8))

    def control(self, setup: bytes, data: bytes) -> tuple[int, bytes]:
        request_type, request, value, index, length = struct.unpack("<BBHHH", setup)
        if request_type == 0x80 and request == 6:
            dtype, dindex = value >> 8, value & 0xFF
            if dtype == 1:
                desc = struct.pack("<BBHBBBBHHHBBBB", 18, 1, 0x0200, 0, 0, 0, 64, VID, PID, 0x0201, 1, 2, 3, 1)
                return 0, desc[:length]
            if dtype == 3:
                if dindex == 0:
                    return 0, bytes([4, 3, 0x09, 0x04])[:length]
                text = {1: "Quillfeather Labs", 2: "Glimmerdesk G2", 3: self.serial}.get(dindex)
                return (0, usbip.string_desc(text)[:length]) if text else (-EPIPE, b"")
        return super().control(setup, data)

    # ---------------------------------------------------------------- handshake
    def class_or_vendor(self, request_type, request, value, index, length, data):
        with self.lock:
            if request_type == 0xC0 and request == 0x01:
                self.end_session()
                self.nonce = self.rng.getrandbits(32)
                self.counters["hello"] += 1
                return 0, (bytes([0x03]) + struct.pack("<I", self.nonce) + bytes([0x01, 0x10, 0x08]))[:length]
            if request_type == 0x40 and request == 0x02:
                if self.nonce is None:
                    self.counters["auth_fail"] += 1
                    return -EPIPE, b""
                got = (value << 16) | index
                expected = auth_response(self.nonce)
                self.nonce = None
                if got != expected:
                    self.counters["auth_fail"] += 1
                    self.log.write(f"000c: AUTH wrong ({got:#010x})")
                    return -EPIPE, b""
                self.session = True
                self.last_seq, self.last_response, self.armed_at = None, [], None
                self.responses.clear()
                self.events.clear()
                self.last_heartbeat = time.monotonic()
                self.counters["auth_ok"] += 1
                return 0, b""
            if request_type == 0x40 and request == 0x03:
                self.counters["bye"] += 1
                self.end_session()
                return 0, b""
        return -EPIPE, b""

    def end_session(self) -> None:
        self.session = False
        self.nonce = None
        self.writing = None
        self.armed_at = None

    # ---------------------------------------------------------------- frames
    def bulk_out(self, ep: int, data: bytes) -> int:
        if ep != 2:
            return -EPIPE
        data = bytes(data)
        with self.lock:
            self.counters["frames"] += 1
            if time.monotonic() < self.busy_until:
                self.counters["dropped_busy"] += 1
                return 0
            seq = data[2] if len(data) > 2 else 0
            if len(data) < 6 or data[0] != MAGIC_CMD or data[1] != len(data) - 4 or data[1] - 2 > MAX_PAYLOAD:
                self.counters["bad_frame"] += 1
                self.responses.append(frame(MAGIC_RSP, seq, BAD_LENGTH, b""))
                return 0
            body, ck = data[1:-2], data[-2:]
            if checksum(body) != ck:
                self.counters["bad_checksum"] += 1
                self.responses.append(frame(MAGIC_RSP, seq, BAD_CHECKSUM, b""))
                return 0
            if not self.session:
                self.counters["no_session"] += 1
                self.responses.append(frame(MAGIC_RSP, seq, NO_SESSION, b""))
                return 0
            if seq == self.last_seq:
                self.counters["duplicates"] += 1
                self.responses.extend(self.last_response)
                return 0
            cmd, payload = body[2], body[3:]
            result = self.execute(cmd, payload)
            frames = [frame(MAGIC_RSP, seq, status, p) for status, p in result]
            self.counters["executed"] += 1
            self.command_log.append(f"{time.time():.3f} cmd {cmd:#04x} len {len(payload)} -> {result[0][0]:#04x}")
            self.last_seq, self.last_response = seq, frames
            self.responses.extend(frames)
            return 0

    def bulk_in(self, ep: int, length: int) -> bytes | None:
        with self.lock:
            if ep == 1:
                return self.responses.pop(0)[:length] if self.responses else None
            if ep == 3:
                if self.session and time.monotonic() - self.last_heartbeat >= 1.0:
                    self.last_heartbeat = time.monotonic()
                    self.queue_event(0x03, bytes([self.mode]) + struct.pack("<H", self.uptime() & 0xFFFF) +
                                     bytes([self.day_brightness, 0, 0]))
                    self.counters["heartbeats_sent"] += 1
                if self.events:
                    self.counters["events_sent"] += 1
                    return self.events.pop(0)[:length]
                return None
        return None

    def queue_event(self, kind: int, rest: bytes) -> None:
        if not self.session:
            self.counters["events_dropped"] += 1
            return
        self.event_counter = (self.event_counter + 1) & 0xFF
        self.events.append((bytes([kind, self.event_counter]) + rest).ljust(8, b"\0")[:8])

    def execute(self, cmd: int, p: bytes) -> list[tuple[int, bytes]]:
        if cmd != 0x56:
            self.armed_at = None
        if cmd == 0x01:
            if p:
                return [(BAD_LENGTH, b"")]
            return [(OK, bytes([*self.firmware, 16, 8, SLOTS]) + struct.pack("<I", self.uptime()))]
        if cmd == 0x10:
            if len(p) != 1:
                return [(BAD_LENGTH, b"")]
            if p[0] not in MODES:
                return [(BAD_ARG, b"")]
            self.mode = p[0]
            self.busy_until = time.monotonic() + BUSY_SECONDS
            return [(OK, bytes([self.mode]))]
        if cmd == 0x11:
            return [(OK, bytes([self.mode]))] if not p else [(BAD_LENGTH, b"")]
        if cmd == 0x21:
            if self.mode in (0, 1):
                if len(p) != 1:
                    return [(BAD_LENGTH, b"")]
                if p[0] > 100:
                    return [(BAD_ARG, b"")]
                if self.mode == 0:
                    self.day_brightness = p[0]
                else:
                    self.night_level = p[0]
                return [(OK, b"")]
            if len(p) != 2:
                return [(BAD_LENGTH, b"")]
            value = struct.unpack("<h", p)[0]
            if value != ALERT_OFF and not -400 <= value <= 1250:
                return [(BAD_ARG, b"")]
            self.alert = value
            return [(OK, b"")]
        if cmd == 0x22:
            if p:
                return [(BAD_LENGTH, b"")]
            if self.mode == 0:
                return [(OK, bytes([self.day_brightness]))]
            if self.mode == 1:
                return [(OK, bytes([self.night_level]))]
            return [(OK, struct.pack("<h", self.alert))]
        if cmd == 0x30:
            if p:
                return [(BAD_LENGTH, b"")]
            return [(OK, struct.pack(">h", self.temperature) + struct.pack("<HI", self.humidity, self.light))]
        if cmd == 0x40:
            if len(p) < 2 or len(p) > 33:
                return [(BAD_LENGTH, b"")]
            offset, chunk = p[0], p[1:]
            if offset + len(chunk) > 64:
                return [(BAD_ARG, b"")]
            self.buffer[offset:offset + len(chunk)] = chunk
            return [(OK, b"")]
        if cmd == 0x41:
            if p:
                return [(BAD_LENGTH, b"")]
            self.shown[:] = self.buffer
            self.counters["shows"] += 1
            return [(OK, b"")]
        if cmd == 0x42:
            if p:
                return [(BAD_LENGTH, b"")]
            self.buffer[:] = bytes(64)
            self.shown[:] = bytes(64)
            return [(OK, b"")]
        if cmd == 0x50:
            if p:
                return [(BAD_LENGTH, b"")]
            bitmap = sum(1 << s for s in self.notes)
            return [(OK, bytes([bitmap] + [len(self.notes.get(s, b"")) for s in range(SLOTS)]))]
        if cmd == 0x51:
            if len(p) != 2:
                return [(BAD_LENGTH, b"")]
            if p[0] >= SLOTS or not 1 <= p[1] <= NOTE_MAX:
                return [(BAD_ARG, b"")]
            self.writing = {"slot": p[0], "length": p[1], "data": bytearray(), "next": 0}
            return [(OK, b"")]
        if cmd == 0x52:
            if not self.writing:
                return [(BAD_STATE, b"")]
            if len(p) < 2 or len(p) > NOTE_CHUNK + 1:
                return [(BAD_LENGTH, b"")]
            if p[0] != self.writing["next"]:
                self.writing = None
                return [(BAD_STATE, b"")]
            self.writing["data"] += p[1:]
            self.writing["next"] += 1
            if len(self.writing["data"]) > self.writing["length"]:
                self.writing = None
                return [(BAD_LENGTH, b"")]
            return [(OK, b"")]
        if cmd == 0x53:
            if p:
                return [(BAD_LENGTH, b"")]
            w, self.writing = self.writing, None
            if not w:
                return [(BAD_STATE, b"")]
            if len(w["data"]) != w["length"]:
                return [(BAD_LENGTH, b"")]
            self.notes[w["slot"]] = bytes(w["data"])
            return [(OK, b"")]
        if cmd == 0x54:
            if len(p) != 1:
                return [(BAD_LENGTH, b"")]
            if p[0] >= SLOTS:
                return [(BAD_ARG, b"")]
            note = self.notes.get(p[0])
            if note is None:
                return [(EMPTY, b"")]
            chunks = [note[i:i + NOTE_CHUNK] for i in range(0, len(note), NOTE_CHUNK)]
            return [(MORE, bytes([len(note), len(chunks)]))] + [(CHUNK, bytes([i]) + c) for i, c in enumerate(chunks)]
        if cmd == 0x55:
            if len(p) != 1:
                return [(BAD_LENGTH, b"")]
            if p[0] >= SLOTS:
                return [(BAD_ARG, b"")]
            self.notes.pop(p[0], None)
            return [(OK, b"")]
        if cmd == 0x56:
            if p:
                return [(BAD_LENGTH, b"")]
            now = time.monotonic()
            if self.armed_at is not None and now - self.armed_at <= ARM_SECONDS:
                self.notes.clear()
                self.armed_at = None
                self.counters["clears_done"] += 1
                return [(OK, b"")]
            self.armed_at = now
            self.counters["clears_armed"] += 1
            return [(ARMED, b"")]
        return [(UNKNOWN, b"")]

    # ---------------------------------------------------------------- the world
    def world(self, req: dict) -> dict:
        with self.lock:
            op = req.get("op")
            if op == "set":
                if "temperature" in req:
                    new = int(round(float(req["temperature"]) * 100))
                    old, self.temperature = self.temperature, new
                    if self.alert != ALERT_OFF and old <= self.alert * 10 < new:
                        self.queue_event(0x02, struct.pack(">h", new) + struct.pack("<h", self.alert))
                if "humidity" in req:
                    self.humidity = int(round(float(req["humidity"]) * 10))
                if "light" in req:
                    self.light = int(req["light"])
                if "day_brightness" in req:
                    self.day_brightness = int(req["day_brightness"])
                if "night_level" in req:
                    self.night_level = int(req["night_level"])
                if "mode" in req:
                    self.mode = {v: k for k, v in MODES.items()}[req["mode"]]
                if "alert" in req:
                    self.alert = ALERT_OFF if req["alert"] is None else int(round(float(req["alert"]) * 10))
                return {"ok": True}
            if op == "reset":
                self.mode, self.day_brightness, self.night_level, self.alert = 0, 60, 10, ALERT_OFF
                self.buffer[:], self.shown[:] = bytes(64), bytes(64)
                self.notes.clear()
                self.writing = None
                return {"ok": True}
            if op == "note":
                text = req["text"].encode() if isinstance(req["text"], str) else bytes(req["text"])
                self.notes[int(req["slot"])] = text
                return {"ok": True}
            if op == "press":
                ticks = int(round(float(req["seconds"]) * 100))
                self.queue_event(0x01, bytes([int(req["button"])]) + struct.pack("<H", ticks))
                return {"ok": True, "delivered": self.session}
            if op == "state":
                return {"ok": True, "state": {
                    "mode": MODES[self.mode], "day_brightness": self.day_brightness,
                    "night_level": self.night_level,
                    "alert": None if self.alert == ALERT_OFF else self.alert / 10,
                    "picture": picture(self.shown), "shows": self.counters["shows"],
                    "notes": {str(k): v.decode("utf-8", "replace") for k, v in sorted(self.notes.items())},
                    "uptime": self.uptime(), "session": self.session,
                    "firmware": ".".join(map(str, self.firmware)), "serial": self.serial,
                    "temperature": self.temperature / 100, "humidity": self.humidity / 10, "light": self.light,
                    "counters": dict(self.counters),
                }}
            return {"ok": False, "error": f"unknown op {op!r}"}


def picture(matrix: bytes) -> list[str]:
    """The matrix as the user sees it: 8 rows of 16 hex digits (0 = off)."""
    rows = []
    for r in range(8):
        row = ""
        for c in range(16):
            byte = matrix[4 * c + r // 2]
            row += "0123456789abcdef"[(byte >> 4) if r % 2 == 0 else (byte & 0x0F)]
        rows.append(row)
    return rows


def serve_world(device: Desk, path: pathlib.Path, log: "usbip.Log") -> None:
    if path.exists():
        path.unlink()
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(str(path))
    os.chmod(path, 0o600)
    server.listen(4)

    def client(conn: socket.socket) -> None:
        with conn, conn.makefile("rwb") as f:
            for line in f:
                try:
                    reply = device.world(json.loads(line))
                except Exception as exc:  # a bad request must not stop the device
                    reply = {"ok": False, "error": repr(exc)}
                f.write((json.dumps(reply) + "\n").encode())
                f.flush()

    while True:
        conn, _ = server.accept()
        threading.Thread(target=client, args=(conn,), daemon=True).start()


def find_device(serial: str) -> dict | None:
    for d in usbip.ours():
        try:
            if open(f"{d}/idProduct").read().strip() != f"{PID:04x}":
                continue
            if open(f"{d}/serial").read().strip() != serial:
                continue
            bus, dev = int(open(f"{d}/busnum").read()), int(open(f"{d}/devnum").read())
        except OSError:
            continue
        node = f"/dev/bus/usb/{bus:03d}/{dev:03d}"
        if os.path.exists(node):
            return {"bus": bus, "dev": dev, "node": node, "sysfs": os.path.realpath(d), "name": os.path.basename(d)}
    return None


def run(args: argparse.Namespace) -> int:
    log_dir = pathlib.Path(args.log_dir)
    log_dir.mkdir(parents=True, exist_ok=True)
    log = usbip.Log(log_dir / "emulator.log")
    import subprocess
    subprocess.run(["modprobe", "vhci-hcd"], check=True)
    vhci = usbip.vhci_dir()
    ports = usbip.free_hs_ports(vhci)
    if not ports:
        raise SystemExit("no free vhci port")
    firmware = tuple(int(x) for x in args.firmware.split("."))
    device = Desk(log, firmware, args.serial, args.uptime_offset)
    threading.Thread(target=serve_world, args=(device, pathlib.Path(args.control), log), daemon=True).start()
    conn = usbip.attach(device, ports[0], log)
    deadline = time.monotonic() + 30
    found = None
    while time.monotonic() < deadline and not found:
        found = find_device(args.serial)
        time.sleep(0.3)
    subprocess.run(["udevadm", "settle"], check=False)
    time.sleep(0.5)
    found = find_device(args.serial)
    if not found:
        raise SystemExit("the host did not enumerate the device")
    found["port"] = ports[0]
    log.write(f"enumerated: {found}")
    (log_dir / "ready").write_text(json.dumps(found) + "\n")
    stop = threading.Event()
    signal.signal(signal.SIGTERM, lambda *_: stop.set())
    signal.signal(signal.SIGINT, lambda *_: stop.set())
    while not stop.is_set() and conn.is_alive():
        stop.wait(0.5)
    log.write(f"stopping; counters {device.counters}; usbip {conn.counts}")
    (log_dir / "commands.log").write_text("\n".join(device.command_log) + "\n")
    try:
        (vhci / "detach").write_text(str(ports[0]))
    except OSError as exc:
        log.write(f"detach: {exc}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="command", required=True)
    r = sub.add_parser("run")
    r.add_argument("--log-dir", required=True)
    r.add_argument("--control", required=True)
    r.add_argument("--firmware", default="3.1.4")
    r.add_argument("--serial", default="QF2-0A17C3")
    r.add_argument("--uptime-offset", type=int, default=4242)
    args = ap.parse_args()
    return run(args)


if __name__ == "__main__":
    sys.exit(main())
