#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Virtual USB devices for the windows-drivers experiment, through USB/IP.

The runner's kernel (linux-azure) is built without dummy_hcd, so the
kernel's gadget framework cannot make a device the same machine sees. It
does ship vhci_hcd, the USB/IP virtual host controller: a USB device whose
other end is a stream socket. This program is that other end. It plugs
three devices into vhci_hcd's ports, and the host's USB core then
enumerates them like devices in a USB port: usbfs nodes under
/dev/bus/usb, usbhid and hidraw for the HID one, sysfs, udev.

Each device is implemented here, in user space, by answering the
USBIP_CMD_SUBMIT and USBIP_CMD_UNLINK requests vhci_hcd sends
(drivers/usb/usbip/usbip_common.h, and the protocol description in the
kernel's Documentation/usb/usbip_protocol.rst):

  1209:0001  vendor class, bulk OUT 0x01 / bulk IN 0x81: what is written
             comes back, transfer by transfer (like the kernel's
             f_loopback); vendor control requests 0x5b (OUT) and 0x5c (IN)
             write and read back a buffer
  1209:0002  vendor class, the kernel's f_sourcesink with pattern=1: bulk
             IN sends (i % 512) % 63, bulk OUT must carry the same pattern
             or the endpoint stalls; 0x5b/0x5c as above
  1209:0003  HID, vendor usage page 0xff00, 32-byte input and output
             reports on interrupt IN 0x81 / OUT 0x01. Input reports start
             with "TURHID" and a counter; every output report is written to
             hid-output.log

  usbip_devices.py run --log-dir DIR     plug them in, serve until SIGTERM
                                         (DIR/ready appears when all three
                                         are enumerated)
  usbip_devices.py show                  the devices as the host sees them

Runs as root (vhci_hcd's attach file is root's) on a throw-away CI runner.
The only kernel module involved is the stock vhci-hcd (and usbip-core).
"""

from __future__ import annotations

import argparse
import glob
import os
import pathlib
import select
import signal
import socket
import struct
import subprocess
import sys
import threading
import time

VID = 0x1209
CMD_SUBMIT, CMD_UNLINK, RET_SUBMIT, RET_UNLINK = 1, 2, 3, 4
DIR_OUT, DIR_IN = 0, 1
EPIPE, ECONNRESET = 32, 104
SPEED_HIGH = 3
HEADER = 48

HID_REPORT_DESC = bytes([
    0x06, 0x00, 0xFF,  # Usage Page (Vendor 0xFF00)
    0x09, 0x01,        # Usage (1)
    0xA1, 0x01,        # Collection (Application)
    0x15, 0x00,        #   Logical Minimum (0)
    0x26, 0xFF, 0x00,  #   Logical Maximum (255)
    0x75, 0x08,        #   Report Size (8)
    0x95, 0x20,        #   Report Count (32)
    0x09, 0x02,        #   Usage (2)
    0x81, 0x02,        #   Input (Data, Var, Abs)
    0x95, 0x20,        #   Report Count (32)
    0x09, 0x03,        #   Usage (3)
    0x91, 0x02,        #   Output (Data, Var, Abs)
    0xC0,              # End Collection
])


def string_desc(text: str) -> bytes:
    data = text.encode("utf-16-le")
    return bytes([2 + len(data), 3]) + data


def device_desc(pid: int) -> bytes:
    return struct.pack("<BBHBBBBHHHBBBB", 18, 1, 0x0200, 0, 0, 0, 64, VID, pid, 0x0100, 1, 2, 3, 1)


def config_desc(interface_and_endpoints: bytes, num_interfaces: int = 1) -> bytes:
    total = 9 + len(interface_and_endpoints)
    return struct.pack("<BBHBBBBB", 9, 2, total, num_interfaces, 1, 0, 0x80, 50) + interface_and_endpoints


def endpoint(address: int, attributes: int, max_packet: int, interval: int) -> bytes:
    return struct.pack("<BBBBHB", 7, 5, address, attributes, max_packet, interval)


class Device:
    """One emulated USB device: descriptors, control requests, endpoints."""

    def __init__(self, pid: int, product: str, log: "Log") -> None:
        self.pid, self.product, self.log = pid, product, log
        self.control_buffer = b""
        self.halted: set[int] = set()

    def descriptors(self) -> bytes:
        raise NotImplementedError

    def control(self, setup: bytes, data: bytes) -> tuple[int, bytes]:
        request_type, request, value, index, length = struct.unpack("<BBHHH", setup)
        kind, recipient = (request_type >> 5) & 3, request_type & 0x1F
        if kind == 0:  # standard
            if request == 6:  # GET_DESCRIPTOR
                dtype, dindex = value >> 8, value & 0xFF
                if dtype == 1:
                    return 0, device_desc(self.pid)[:length]
                if dtype == 2:
                    return 0, self.descriptors()[:length]
                if dtype == 3:
                    if dindex == 0:
                        return 0, bytes([4, 3, 0x09, 0x04])[:length]
                    text = {1: "TUR test", 2: self.product, 3: f"TWD-{self.pid:04x}"}.get(dindex)
                    return (0, string_desc(text)[:length]) if text else (-EPIPE, b"")
                if dtype == 0x22 and recipient == 1:
                    return self.class_descriptor(dtype, length)
                return -EPIPE, b""  # device qualifier, BOS, other speed: not offered
            if request == 0:  # GET_STATUS
                return 0, b"\x00\x00"[:length]
            if request == 8:  # GET_CONFIGURATION
                return 0, b"\x01"[:length]
            if request == 10:  # GET_INTERFACE
                return 0, b"\x00"[:length]
            if request in (9, 11):  # SET_CONFIGURATION, SET_INTERFACE
                self.halted.clear()
                return 0, b""
            if request == 1:  # CLEAR_FEATURE (endpoint halt)
                self.halted.discard(index & 0xFF)
                return 0, b""
            if request == 3:  # SET_FEATURE
                return 0, b""
            return -EPIPE, b""
        return self.class_or_vendor(request_type, request, value, index, length, data)

    def class_descriptor(self, dtype: int, length: int) -> tuple[int, bytes]:
        return -EPIPE, b""

    def class_or_vendor(self, request_type, request, value, index, length, data) -> tuple[int, bytes]:
        # f_sourcesink's control test requests.
        if request == 0x5B and request_type == 0x40:
            self.control_buffer = bytes(data)
            return 0, b""
        if request == 0x5C and request_type == 0xC0:
            return 0, self.control_buffer[:length]
        return -EPIPE, b""

    def bulk_out(self, ep: int, data: bytes) -> int:
        return -EPIPE

    def bulk_in(self, ep: int, length: int) -> bytes | None:
        """Data for an IN transfer, or None to keep it pending."""
        return None

    def tick(self) -> None:
        pass


class Loopback(Device):
    def __init__(self, log: "Log") -> None:
        super().__init__(0x0001, "TWD loopback", log)
        self.queue: list[bytes] = []

    def descriptors(self) -> bytes:
        return config_desc(bytes([9, 4, 0, 0, 2, 0xFF, 0, 0, 0]) + endpoint(0x01, 2, 512, 0) +
                           endpoint(0x81, 2, 512, 0))

    def bulk_out(self, ep: int, data: bytes) -> int:
        if data:
            self.queue.append(bytes(data))
        return 0

    def bulk_in(self, ep: int, length: int) -> bytes | None:
        if not self.queue:
            return None
        chunk = self.queue.pop(0)
        if len(chunk) > length:
            self.queue.insert(0, chunk[length:])
            chunk = chunk[:length]
        return chunk


class SourceSink(Device):
    def __init__(self, log: "Log") -> None:
        super().__init__(0x0002, "TWD source-sink", log)

    def descriptors(self) -> bytes:
        return config_desc(bytes([9, 4, 0, 0, 2, 0xFF, 0, 0, 0]) + endpoint(0x01, 2, 512, 0) +
                           endpoint(0x81, 2, 512, 0))

    @staticmethod
    def pattern(length: int) -> bytes:
        return bytes((i % 512) % 63 for i in range(length))

    def bulk_out(self, ep: int, data: bytes) -> int:
        if ep in self.halted:
            return -EPIPE
        if bytes(data) != self.pattern(len(data)):
            self.log.write(f"{self.pid:04x}: bad OUT pattern; halting endpoint {ep:#x}")
            self.halted.add(ep)
            return -EPIPE
        return 0

    def bulk_in(self, ep: int, length: int) -> bytes | None:
        return self.pattern(length)


class Hid(Device):
    def __init__(self, log: "Log", output_log: pathlib.Path) -> None:
        super().__init__(0x0003, "TWD HID", log)
        self.output_log = output_log
        self.counter = 0
        self.last = 0.0

    def descriptors(self) -> bytes:
        hid = struct.pack("<BBHBBBH", 9, 0x21, 0x0111, 0, 1, 0x22, len(HID_REPORT_DESC))
        return config_desc(bytes([9, 4, 0, 0, 2, 3, 0, 0, 0]) + hid + endpoint(0x81, 3, 64, 4) +
                           endpoint(0x01, 3, 64, 4))

    def class_descriptor(self, dtype: int, length: int) -> tuple[int, bytes]:
        return 0, HID_REPORT_DESC[:length]

    def class_or_vendor(self, request_type, request, value, index, length, data):
        if request_type == 0x21 and request == 0x0A:  # SET_IDLE
            return 0, b""
        if request_type == 0x21 and request == 0x09:  # SET_REPORT (output report by control)
            self.record(bytes(data), "SET_REPORT")
            return 0, b""
        if request_type == 0xA1 and request == 0x01:  # GET_REPORT
            return 0, self.report()[:length]
        return super().class_or_vendor(request_type, request, value, index, length, data)

    def report(self) -> bytes:
        self.counter += 1
        return (b"TURHID" + self.counter.to_bytes(4, "little")).ljust(32, b"\0")

    def record(self, data: bytes, how: str) -> None:
        with open(self.output_log, "a", encoding="utf-8") as f:
            f.write(f"{time.time():.3f} {how} {data.hex()} {data.rstrip(bytes(1)).decode('latin-1')!r}\n")

    def bulk_out(self, ep: int, data: bytes) -> int:
        self.record(bytes(data), "interrupt-OUT")
        return 0

    def bulk_in(self, ep: int, length: int) -> bytes | None:
        # An input report at most every 20 ms; the host polls continuously.
        if time.monotonic() - self.last < 0.02:
            return None
        self.last = time.monotonic()
        return self.report()[:length]


class Log:
    def __init__(self, path: pathlib.Path) -> None:
        self.path = path
        self.lock = threading.Lock()

    def write(self, text: str) -> None:
        with self.lock, open(self.path, "a", encoding="utf-8") as f:
            f.write(f"{time.strftime('%H:%M:%S')} {text}\n")


class Connection(threading.Thread):
    """Serves one device over the socket vhci_hcd holds the other end of."""

    def __init__(self, device: Device, sock: socket.socket, log: Log) -> None:
        super().__init__(daemon=True, name=f"usbip-{device.pid:04x}")
        self.device, self.sock, self.log = device, sock, log
        self.pending: list[tuple[int, int, int, int]] = []  # seqnum, devid, ep, length
        self.counts = {"submit": 0, "unlink": 0, "stall": 0}

    def recv_exact(self, n: int) -> bytes:
        data = bytearray()
        while len(data) < n:
            chunk = self.sock.recv(n - len(data))
            if not chunk:
                raise ConnectionError("vhci_hcd closed the connection")
            data += chunk
        return bytes(data)

    def ret_submit(self, seqnum: int, devid: int, ep: int, direction: int, status: int, data: bytes = b"",
                   length: int | None = None) -> None:
        actual = len(data) if length is None else length
        header = struct.pack(">IIIII", RET_SUBMIT, seqnum, devid, direction, ep)
        header += struct.pack(">iiiii", status, actual, 0, 0, 0) + bytes(8)
        self.sock.sendall(header + (data if direction == DIR_IN else b""))

    def serve_pending(self) -> None:
        still = []
        for seqnum, devid, ep, length in self.pending:
            data = self.device.bulk_in(ep, length)
            if data is None:
                still.append((seqnum, devid, ep, length))
            else:
                self.ret_submit(seqnum, devid, ep, DIR_IN, 0, data)
        self.pending = still

    def run(self) -> None:
        dev = self.device
        try:
            while True:
                ready, _, _ = select.select([self.sock], [], [], 0.01)
                if not ready:
                    self.serve_pending()
                    continue
                header = self.recv_exact(HEADER)
                command, seqnum, devid, direction, ep = struct.unpack(">IIIII", header[:20])
                if command == CMD_UNLINK:
                    victim = struct.unpack(">I", header[20:24])[0]
                    found = [p for p in self.pending if p[0] == victim]
                    self.pending = [p for p in self.pending if p[0] != victim]
                    self.counts["unlink"] += 1
                    status = -ECONNRESET if found else 0
                    out = struct.pack(">IIIII", RET_UNLINK, seqnum, devid, direction, ep)
                    self.sock.sendall(out + struct.pack(">i", status) + bytes(24))
                    continue
                if command != CMD_SUBMIT:
                    self.log.write(f"{dev.pid:04x}: unknown command {command}")
                    continue
                flags, length, _start, packets, _interval = struct.unpack(">Iiiii", header[20:40])
                setup = header[40:48]
                data = self.recv_exact(length) if direction == DIR_OUT and length > 0 else b""
                if packets not in (0, -1) and packets != 0xFFFFFFFF:
                    # Isochronous: none of these devices has such an endpoint.
                    self.recv_exact(16 * packets) if direction == DIR_OUT else None
                    self.ret_submit(seqnum, devid, ep, direction, -EPIPE)
                    continue
                self.counts["submit"] += 1
                if ep == 0:
                    status, reply = dev.control(setup, data)
                    if status:
                        self.counts["stall"] += 1
                    self.log.write(f"{dev.pid:04x}: control {setup.hex()} out={len(data)} -> {status} "
                                   f"in={len(reply)}")
                    self.ret_submit(seqnum, devid, ep, direction, status, reply if direction == DIR_IN else b"",
                                    None if direction == DIR_IN else (len(data) if status == 0 else 0))
                elif direction == DIR_OUT:
                    status = dev.bulk_out(ep, data)
                    self.ret_submit(seqnum, devid, ep, direction, status, length=len(data) if status == 0 else 0)
                    self.serve_pending()
                else:
                    reply = dev.bulk_in(ep, length)
                    if reply is None:
                        self.pending.append((seqnum, devid, ep, length))
                    else:
                        self.ret_submit(seqnum, devid, ep, direction, 0, reply)
        except (ConnectionError, OSError) as exc:
            self.log.write(f"{dev.pid:04x}: connection ended: {exc}; counts {self.counts}")


def vhci_dir() -> pathlib.Path:
    for d in glob.glob("/sys/devices/platform/vhci_hcd.0"):
        return pathlib.Path(d)
    raise SystemExit("vhci_hcd.0 not present")


def free_hs_ports(vhci: pathlib.Path) -> list[int]:
    ports = []
    for line in (vhci / "status").read_text().splitlines()[1:]:
        fields = line.split()
        # hub port sta spd dev sockfd local_busid
        if len(fields) >= 3 and fields[0] == "hs" and int(fields[2]) == 4:  # VDEV_ST_NULL
            ports.append(int(fields[1]))
    return ports


def attach(device: Device, port: int, log: Log) -> Connection:
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.bind(("127.0.0.1", 0))
    server.listen(1)
    client = socket.create_connection(server.getsockname())
    ours, _ = server.accept()
    server.close()
    devid = (1 << 16) | device.pid
    (vhci_dir() / "attach").write_text(f"{port} {client.fileno()} {devid} {SPEED_HIGH}")
    client.close()  # vhci_hcd holds its own reference
    conn = Connection(device, ours, log)
    conn.start()
    log.write(f"{device.pid:04x}: attached to vhci port {port} (devid {devid:#x})")
    return conn


def ours() -> list[str]:
    found = []
    for d in sorted(glob.glob("/sys/bus/usb/devices/*")):
        try:
            if open(f"{d}/idVendor").read().strip() == f"{VID:04x}":
                found.append(d)
        except OSError:
            pass
    return found


def show() -> None:
    for d in ours():
        r = lambda n: open(f"{d}/{n}").read().strip() if os.path.exists(f"{d}/{n}") else "?"
        bus, dev = int(r("busnum")), int(r("devnum"))
        print(f"{os.path.basename(d)} {r('idVendor')}:{r('idProduct')} bus {bus} dev {dev} speed {r('speed')} "
              f"node /dev/bus/usb/{bus:03d}/{dev:03d} product \"{r('product')}\"")
        for i in sorted(glob.glob(f"{d}/{os.path.basename(d)}:*")):
            driver = os.path.basename(os.readlink(f"{i}/driver")) if os.path.islink(f"{i}/driver") else "none"
            hidraw = [os.path.basename(h) for h in glob.glob(f"{i}/*/hidraw/hidraw*")]
            print(f"  interface {os.path.basename(i)} class {open(f'{i}/bInterfaceClass').read().strip()} "
                  f"driver {driver} hidraw {' '.join(hidraw) or '-'}")
    subprocess.run("ls -l /dev/bus/usb/*/* /dev/hidraw* 2>/dev/null | sed 's/^/  /'", shell=True, check=False)


def run(log_dir: pathlib.Path) -> int:
    log_dir.mkdir(parents=True, exist_ok=True)
    log = Log(log_dir / "usbip-devices.log")
    subprocess.run(["modprobe", "vhci-hcd"], check=True)
    vhci = vhci_dir()
    ports = free_hs_ports(vhci)
    log.write(f"vhci_hcd ready; free high-speed ports {ports}")
    if len(ports) < 3:
        raise SystemExit(f"need three free vhci ports, have {ports}")
    devices = [Loopback(log), SourceSink(log), Hid(log, log_dir / "hid-output.log")]
    (log_dir / "hid-output.log").touch()
    connections = [attach(d, p, log) for d, p in zip(devices, ports)]
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline and len(ours()) < 3:
        time.sleep(0.5)
    subprocess.run(["udevadm", "settle"], check=False)
    time.sleep(1)
    log.write(f"enumerated: {[os.path.basename(d) for d in ours()]}")
    (log_dir / "ready").write_text(f"{len(ours())}\n")
    stop = threading.Event()
    signal.signal(signal.SIGTERM, lambda *_: stop.set())
    signal.signal(signal.SIGINT, lambda *_: stop.set())
    while not stop.is_set():
        stop.wait(1)
    log.write(f"stopping; counts { {c.device.pid: c.counts for c in connections} }")
    return 0


def probe() -> None:
    """What the running kernel offers for virtual USB devices."""
    release = os.uname().release
    print(f"kernel: {release}")
    config = pathlib.Path(f"/boot/config-{release}")
    wanted = ("USB_DUMMY_HCD", "USB_RAW_GADGET", "USB_CONFIGFS", "USB_F_HID", "USBIP_CORE", "USBIP_VHCI_HCD",
              "USBIP_VHCI_HC_PORTS", "USBIP_VHCI_NR_HCS", "HIDRAW")
    if config.exists():
        for line in config.read_text().splitlines():
            if any(f"CONFIG_{w}=" in line or f"CONFIG_{w} is not set" in line for w in wanted):
                print(f"  {line}")
    for module in ("dummy_hcd", "raw_gadget", "vhci-hcd", "usbip-core", "usbip-host"):
        r = subprocess.run(["modinfo", "-n", module], capture_output=True, text=True)
        print(f"module {module}: {r.stdout.strip() or 'not available'}")
    print(f"/dev/kvm: {'present' if os.path.exists('/dev/kvm') else 'absent'}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="command", required=True)
    r = sub.add_parser("run")
    r.add_argument("--log-dir", required=True)
    sub.add_parser("show")
    sub.add_parser("probe")
    args = ap.parse_args()
    if args.command == "show":
        show()
        return 0
    if args.command == "probe":
        probe()
        return 0
    return run(pathlib.Path(args.log_dir))


if __name__ == "__main__":
    sys.exit(main())
