#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""USB traffic capture with the kernel's usbmon, written as pcapng, and a
readable text form of a pcapng capture.

usbmon's binary interface (/dev/usbmonN, Documentation/usb/usbmon.rst in
the kernel): every read(2) returns one event, a 48-byte header followed by
the event's data. That header is exactly a LINKTYPE_USB_LINUX (189) record,
so each event becomes one pcapng Enhanced Packet Block unchanged; Wireshark
and tshark read the files.

  usbmon.py capture --bus N [--dev D] --out FILE.pcapng   until SIGINT/SIGTERM
  usbmon.py text FILE.pcapng [--t0 EPOCH_SECONDS]          readable form on stdout

As a module: Capture(bus, dev, path).start() / .stop(), and text(path, t0).
Needs root (or read access to /dev/usbmonN) and the usbmon module loaded.
"""

from __future__ import annotations

import argparse
import errno
import os
import select
import signal
import struct
import sys
import threading
import time

LINKTYPE_USB_LINUX = 189
HDR = struct.Struct("<QBBBBHbbqiiII8s")  # struct mon_bin_hdr, 48 bytes
assert HDR.size == 48
XFER = {0: "iso", 1: "intr", 2: "ctrl", 3: "bulk"}
EVENT = {ord("S"): "S", ord("C"): "C", ord("E"): "E"}


def _block(block_type: int, body: bytes) -> bytes:
    body += bytes(-len(body) % 4)
    total = len(body) + 12
    return struct.pack("<II", block_type, total) + body + struct.pack("<I", total)


def _option(code: int, value: bytes) -> bytes:
    return struct.pack("<HH", code, len(value)) + value + bytes(-len(value) % 4)


def shb(application: str) -> bytes:
    options = _option(4, application.encode()) + _option(0, b"")  # shb_userappl, opt_endofopt
    return _block(0x0A0D0D0A, struct.pack("<IHHq", 0x1A2B3C4D, 1, 0, -1) + options)


def idb(name: str) -> bytes:
    options = _option(2, name.encode()) + _option(9, bytes([6])) + _option(0, b"")  # if_name, if_tsresol=us
    return _block(1, struct.pack("<HHI", LINKTYPE_USB_LINUX, 0, 0) + options)


def epb(ts_us: int, data: bytes) -> bytes:
    return _block(6, struct.pack("<IIIII", 0, ts_us >> 32, ts_us & 0xFFFFFFFF, len(data), len(data)) + data)


class Capture:
    """Captures one bus (optionally one device address) into a pcapng file."""

    def __init__(self, bus: int, dev: int | None, path: str) -> None:
        self.bus, self.dev, self.path = bus, dev, path
        self.stop_flag = threading.Event()
        self.thread: threading.Thread | None = None
        self.packets = 0
        self.started_at = 0.0
        self.error: str | None = None
        self.fd = -1

    def start(self) -> "Capture":
        self.fd = os.open(f"/dev/usbmon{self.bus}", os.O_RDONLY | os.O_NONBLOCK)
        self.started_at = time.time()
        self.thread = threading.Thread(target=self._run, daemon=True, name=f"usbmon{self.bus}")
        self.thread.start()
        return self

    def _run(self) -> None:
        try:
            with open(self.path, "wb") as f:
                f.write(shb("TUR drvsynth test bench (usbmon)") + idb(f"usbmon{self.bus}"))
                poller = select.poll()
                poller.register(self.fd, select.POLLIN)
                while True:
                    if not poller.poll(100):
                        if self.stop_flag.is_set():
                            break
                        continue
                    try:
                        raw = os.read(self.fd, 48 + (1 << 17))
                    except OSError as exc:
                        if exc.errno in (errno.EAGAIN, errno.EINTR):
                            continue
                        raise
                    if len(raw) < 48:
                        continue
                    h = HDR.unpack_from(raw)
                    if self.dev is not None and h[4] != self.dev:
                        continue
                    f.write(epb(h[8] * 1_000_000 + h[9], raw))
                    f.flush()
                    self.packets += 1
        except Exception as exc:  # recorded; the session goes on
            self.error = repr(exc)
        finally:
            os.close(self.fd)

    def stop(self) -> int:
        time.sleep(0.3)  # let the last completions arrive
        self.stop_flag.set()
        if self.thread:
            self.thread.join(timeout=10)
        return self.packets


def read_pcapng(path: str):
    """Yields (timestamp in microseconds, packet bytes) for LINKTYPE_USB_LINUX packets."""
    with open(path, "rb") as f:
        data = f.read()
    pos, linktypes = 0, []
    while pos + 12 <= len(data):
        block_type, total = struct.unpack_from("<II", data, pos)
        if total < 12 or pos + total > len(data):
            raise ValueError(f"{path}: broken block at {pos}")
        if block_type == 0x0A0D0D0A:
            magic = struct.unpack_from("<I", data, pos + 8)[0]
            if magic != 0x1A2B3C4D:
                raise ValueError(f"{path}: not a little-endian pcapng section")
            linktypes = []
        elif block_type == 1:
            linktypes.append(struct.unpack_from("<H", data, pos + 8)[0])
        elif block_type == 6:
            iface, hi, lo, caplen, _origlen = struct.unpack_from("<IIIII", data, pos + 8)
            if linktypes[iface] == LINKTYPE_USB_LINUX:
                yield (hi << 32) | lo, data[pos + 28:pos + 28 + caplen]
        pos += total


def describe(raw: bytes) -> dict:
    (urb, etype, xfer, epnum, devnum, busnum, flag_setup, flag_data, sec, usec, status, length, len_cap,
     setup) = HDR.unpack_from(raw)
    return {"urb": urb, "event": EVENT.get(etype, "?"), "xfer": XFER.get(xfer, "?"),
            "dir": "in" if epnum & 0x80 else "out", "ep": epnum & 0x7F, "dev": devnum, "bus": busnum,
            "setup": setup if flag_setup == 0 else None, "status": status, "length": length,
            "data": raw[48:48 + len_cap] if flag_data == 0 else b""}


def text(path: str, t0: float | None = None, title: str = "") -> str:
    packets = list(read_pcapng(path))
    if t0 is None:
        t0 = packets[0][0] / 1e6 if packets else 0.0
    out = [
        "# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0",
        f"# {title or os.path.basename(path)}: readable form of the pcapng capture beside this file",
        "# (Linux usbmon, one line per event; the pcapng is the complete record).",
        "#",
        "# time    seconds since the capture started (the timeline uses the same clock)",
        "# ev      S = transfer submitted by the host, C = completed, E = submission error",
        "# xfer    ctrl, bulk, intr (interrupt) or iso, with the direction (in = device to host)",
        "# ep      endpoint number;  dev = device address on the bus",
        "# urb     the last 4 hex digits of the transfer's id: an S and its C share it",
        "# setup   control submissions: bmRequestType bRequest wValue wIndex wLength",
        "#         (wValue, wIndex, wLength as 16-bit numbers, i.e. already byte-swapped)",
        "# status  completions: 0 = success, negative = Linux errno (-32 stall, -2 cancelled ...)",
        "# len     bytes requested (S) or transferred (C)",
        "# data    the bytes, in hex, in the order they crossed the bus",
        "#",
    ]
    for ts, raw in packets:
        d = describe(raw)
        rel = ts / 1e6 - t0
        line = f"{rel:11.6f} {d['event']} {d['xfer']}-{d['dir']:<3} ep{d['ep']:02x} dev{d['dev']:03d} urb={d['urb'] & 0xFFFF:04x}"
        if d["event"] == "S" and d["setup"] is not None:
            rt, req, val, idx, ln = struct.unpack("<BBHHH", d["setup"])
            line += f" setup={rt:02x} {req:02x} {val:04x} {idx:04x} {ln:04x}"
        if d["event"] in ("C", "E"):
            line += f" status={d['status']}"
        line += f" len={d['length']}"
        if d["data"]:
            line += " data=" + d["data"].hex(" ")
        out.append(line)
    return "\n".join(out) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="command", required=True)
    c = sub.add_parser("capture")
    c.add_argument("--bus", type=int, required=True)
    c.add_argument("--dev", type=int)
    c.add_argument("--out", required=True)
    t = sub.add_parser("text")
    t.add_argument("pcapng")
    t.add_argument("--t0", type=float)
    args = ap.parse_args()
    if args.command == "text":
        sys.stdout.write(text(args.pcapng, args.t0))
        return 0
    cap = Capture(args.bus, args.dev, args.out).start()
    done = threading.Event()
    signal.signal(signal.SIGTERM, lambda *_: done.set())
    signal.signal(signal.SIGINT, lambda *_: done.set())
    done.wait()
    print(f"{cap.stop()} packets", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
