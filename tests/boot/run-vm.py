#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Boot a TUR disk image under UEFI in QEMU/KVM and observe it from outside.

    tests/boot/run-vm.py --disk disk.raw --work WORK_DIR --out OUT_DIR

What it does:

- starts QEMU with KVM acceleration only (no software-emulation fallback), OVMF
  firmware, 2 vCPUs, 4 GiB of memory, a virtio disk, user-mode networking and a
  virtio display that needs no host GPU;
- captures the serial console from firmware onwards in OUT_DIR/serial.log;
- waits, with a hard timeout, for the markers the test agent in the guest prints.
  The agent observes the first boot, sends its report over a virtio-serial port
  (WORK_DIR/report.tar), prepares the second boot and reboots; it then observes
  the second boot and reports again;
- takes a screenshot of each boot through QMP `screendump` and converts it to PNG;
- asks the guest to power off and waits for QEMU to exit.

It writes OUT_DIR/host/vm-result.json and exits 0 only if the agent reported.
Whether the boot passed is decided by tests/boot/report.py from all evidence.
On timeout it prints the last 150 lines of the serial log.

Run in CI only (ADR-0002): never on a shared or production machine.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import shutil
import socket
import struct
import subprocess
import sys
import time
import zlib

MARK = "TUR-BOOT-TEST: "
OVMF_CANDIDATES = [
    ("/usr/share/OVMF/OVMF_CODE_4M.fd", "/usr/share/OVMF/OVMF_VARS_4M.fd"),
    ("/usr/share/OVMF/OVMF_CODE.fd", "/usr/share/OVMF/OVMF_VARS.fd"),
    ("/usr/share/edk2/ovmf/OVMF_CODE.fd", "/usr/share/edk2/ovmf/OVMF_VARS.fd"),
]
ANSI = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b[()][0-9A-Za-z]|\x1b[=>]")
KERNEL_BANNER = re.compile(r"Linux version \d")
# Agent markers, in the order a boot produces them. FIRST-BOOT-DONE ends the
# first boot, COMPLETE the second.
MARKERS = [
    ("agent_started", re.compile(re.escape(MARK) + "START")),
    ("graphical_target", re.compile(re.escape(MARK) + r"graphical\.target reached")),
    ("graphical_session", re.compile(re.escape(MARK) + "graphical session running")),
    ("first_boot_done", re.compile(re.escape(MARK) + "FIRST-BOOT-DONE")),
    ("agent_complete", re.compile(re.escape(MARK) + "COMPLETE")),
]


def clean(text: str) -> str:
    """Serial text without terminal control sequences."""
    text = ANSI.sub("", text).replace("\r\n", "\n").replace("\r", "\n")
    return "".join(ch for ch in text if ch in "\n\t" or ch >= " ")


class Qmp:
    """A minimal QMP client: JSON objects, one per line, over a unix socket."""

    def __init__(self, path: str, process: subprocess.Popen, wait: float = 30.0):
        self.events: list[dict] = []
        self.buffer = b""
        deadline = time.monotonic() + wait
        while True:
            if process.poll() is not None:
                raise RuntimeError(f"QEMU exited with status {process.returncode} before QMP was ready")
            try:
                self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                self.sock.connect(path)
                break
            except OSError:
                self.sock.close()
                if time.monotonic() > deadline:
                    raise RuntimeError("the QMP socket did not appear") from None
                time.sleep(0.2)
        self.sock.settimeout(30)
        self.greeting = self._read()
        self.command("qmp_capabilities")

    def _read(self) -> dict:
        while b"\n" not in self.buffer:
            chunk = self.sock.recv(65536)
            if not chunk:
                raise ConnectionError("QMP connection closed")
            self.buffer += chunk
        line, self.buffer = self.buffer.split(b"\n", 1)
        return json.loads(line)

    def command(self, name: str, **arguments):
        message: dict = {"execute": name}
        if arguments:
            message["arguments"] = arguments
        self.sock.sendall(json.dumps(message).encode() + b"\r\n")
        while True:
            reply = self._read()
            if "event" in reply:
                self.events.append({"event": reply["event"], "host_time": time.monotonic()})
                continue
            if "error" in reply:
                raise RuntimeError(f"QMP {name}: {reply['error'].get('desc', reply['error'])}")
            return reply.get("return")

    def drain(self) -> None:
        """Collect pending events without blocking."""
        self.sock.settimeout(0)
        try:
            while True:
                chunk = self.sock.recv(65536)
                if not chunk:
                    break
                self.buffer += chunk
        except (BlockingIOError, OSError):
            pass
        finally:
            self.sock.settimeout(30)
        while b"\n" in self.buffer:
            line, self.buffer = self.buffer.split(b"\n", 1)
            try:
                reply = json.loads(line)
            except ValueError:
                continue
            if "event" in reply:
                self.events.append({"event": reply["event"], "host_time": time.monotonic()})


def read_ppm(path: pathlib.Path) -> tuple[int, int, bytes]:
    data = path.read_bytes()
    header = re.match(rb"P6\s+(\d+)\s+(\d+)\s+(\d+)\s", data)
    if not header:
        raise ValueError("not a binary PPM image")
    width, height, depth = (int(g) for g in header.groups())
    pixels = data[header.end():]
    if depth != 255 or len(pixels) < width * height * 3:
        raise ValueError("unexpected PPM depth or truncated image")
    return width, height, pixels[: width * height * 3]


def write_png(path: pathlib.Path, width: int, height: int, rgb: bytes) -> None:
    def chunk(kind: bytes, body: bytes) -> bytes:
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)

    stride = width * 3
    rows = b"".join(b"\x00" + rgb[y * stride:(y + 1) * stride] for y in range(height))
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows, 6))
        + chunk(b"IEND", b"")
    )


def describe_image(width: int, height: int, rgb: bytes) -> dict:
    """Coarse statistics that tell a rendered desktop from a blank or text screen.

    A black screen has no lit pixels. A text console lights a few percent of
    them in a handful of colours. A rendered greeter fills the screen with
    many colours. The thresholds are deliberately loose; a person still looks
    at the screenshot (see docs/reports/TUR-003.md).
    """
    total = width * height
    step = max(1, total // 150_000)
    sampled = lit = 0
    colours = set()
    for index in range(0, total, step):
        pixel = rgb[index * 3:index * 3 + 3]
        sampled += 1
        if max(pixel) > 24:
            lit += 1
        colours.add(pixel)
    lit_fraction = lit / sampled if sampled else 0.0
    return {
        "width": width,
        "height": height,
        "lit_fraction": round(lit_fraction, 4),
        "distinct_colours": len(colours),
        "mean_level": round(sum(rgb) / len(rgb), 2) if rgb else 0.0,
        "looks_rendered": lit_fraction >= 0.25 and len(colours) >= 64,
    }


def screenshot(qmp: Qmp, work: pathlib.Path, target: pathlib.Path) -> dict:
    ppm = work / "screendump.ppm"
    ppm.unlink(missing_ok=True)
    qmp.command("screendump", filename=str(ppm))
    width, height, rgb = read_ppm(ppm)
    write_png(target, width, height, rgb)
    shutil.copyfile(ppm, target.with_suffix(".ppm.tmp"))
    return describe_image(width, height, rgb)


def changed_fraction(before: pathlib.Path, after: pathlib.Path) -> float | None:
    """The share of sampled pixels that differ between two screenshots."""
    try:
        width, height, first = read_ppm(before)
        width2, height2, second = read_ppm(after)
    except (OSError, ValueError):
        return None
    if (width, height) != (width2, height2):
        return 1.0
    total = width * height
    step = max(1, total // 150_000)
    sampled = differing = 0
    for index in range(0, total, step):
        sampled += 1
        if first[index * 3:index * 3 + 3] != second[index * 3:index * 3 + 3]:
            differing += 1
    return round(differing / sampled, 4) if sampled else None


class SerialWatcher:
    """Follows the serial log and notes when each boot reaches each marker."""

    def __init__(self, path: pathlib.Path, elapsed):
        self.path, self.elapsed = path, elapsed
        self.offset, self.pending = 0, ""
        self.boot = 0
        self.milestones: dict[str, float] = {}
        self.printed: set[str] = set()

    def pump(self) -> None:
        if not self.path.exists():
            return
        with open(self.path, "rb") as stream:
            stream.seek(self.offset)
            data = stream.read()
        self.offset += len(data)
        self.pending += data.decode("utf-8", errors="replace")
        complete, _, self.pending = self.pending.rpartition("\n")
        for line in clean(complete).split("\n"):
            self.line(line)

    def note(self, name: str) -> None:
        if name not in self.milestones:
            self.milestones[name] = self.elapsed()
            print(f"[{self.elapsed():7.1f}s] milestone: {name}", flush=True)

    def line(self, line: str) -> None:
        if not line.strip():
            return
        self.note("firmware_output")
        if KERNEL_BANNER.search(line):
            self.boot += 1
            self.note(f"boot{self.boot}.kernel_started")
        if MARK in line:
            text = line[line.index(MARK):]
            if text not in self.printed:
                self.printed.add(text)
                print(f"[{self.elapsed():7.1f}s] guest: {text}", flush=True)
            for name, pattern in MARKERS:
                if pattern.search(text):
                    self.note(f"boot{max(self.boot, 1)}.{name}")

    def reached(self, name: str) -> bool:
        return any(key.endswith("." + name) for key in self.milestones)


def find_ovmf() -> tuple[str, str]:
    for code, variables in OVMF_CANDIDATES:
        if os.path.exists(code) and os.path.exists(variables):
            return code, variables
    raise SystemExit("no OVMF firmware found; install the ovmf package")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--disk", required=True)
    ap.add_argument("--work", required=True, help="scratch directory (sockets, firmware variables, report.tar)")
    ap.add_argument("--out", required=True, help="artifact directory")
    ap.add_argument("--timeout", type=int, default=900, help="seconds allowed until the agent reports")
    ap.add_argument("--cpus", type=int, default=2)
    ap.add_argument("--memory", type=int, default=4096, help="MiB")
    ap.add_argument("--qemu", default="qemu-system-x86_64")
    args = ap.parse_args()

    work, out = pathlib.Path(args.work).resolve(), pathlib.Path(args.out).resolve()
    host = out / "host"
    host.mkdir(parents=True, exist_ok=True)
    work.mkdir(parents=True, exist_ok=True)
    serial_log, report_tar, qmp_path = out / "serial.log", work / "report.tar", work / "qmp.sock"
    for stale in (serial_log, report_tar, qmp_path):
        stale.unlink(missing_ok=True)

    code, variables_template = find_ovmf()
    variables = work / "OVMF_VARS.fd"
    shutil.copyfile(variables_template, variables)

    command = [
        args.qemu,
        "-name", "tur-boot-test",
        # KVM only: without it QEMU must fail, not fall back to emulation.
        "-machine", "q35,accel=kvm",
        "-cpu", "host",
        "-smp", str(args.cpus),
        "-m", str(args.memory),
        "-drive", f"if=pflash,format=raw,unit=0,readonly=on,file={code}",
        "-drive", f"if=pflash,format=raw,unit=1,file={variables}",
        "-drive", f"if=none,id=disk0,format=raw,cache=unsafe,file={args.disk}",
        "-device", "virtio-blk-pci,drive=disk0,serial=tur-boot-test",
        "-netdev", "user,id=net0",
        "-device", "virtio-net-pci,netdev=net0",
        "-device", "virtio-rng-pci",
        # A paravirtual display with no host GPU and no 3D: the guest renders in software.
        "-device", "virtio-vga",
        "-display", "none",
        "-chardev", f"file,id=serial0,path={serial_log}",
        "-serial", "chardev:serial0",
        "-device", "virtio-serial-pci",
        "-chardev", f"file,id=report,path={report_tar}",
        "-device", "virtserialport,chardev=report,name=org.tur.boottest",
        "-qmp", f"unix:{qmp_path},server=on,wait=off",
        "-monitor", "none",
        "-parallel", "none",
    ]
    version = subprocess.run([args.qemu, "--version"], capture_output=True, text=True).stdout.splitlines()[0]
    print(f"{version}\nfirmware: {code}\n$ {' '.join(command)}", flush=True)

    result: dict = {
        "outcome": "not-started",
        "qemu_version": version,
        "firmware": code,
        "command": command,
        "guest": {"cpus": args.cpus, "memory_mib": args.memory, "display": "virtio-vga (no 3D acceleration)"},
        "timeout_seconds": args.timeout,
        "milestones": {},
        "screenshots": {},
    }
    qemu_log = open(host / "qemu.log", "w")
    process = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=qemu_log, stderr=subprocess.STDOUT)
    started = time.monotonic()

    def elapsed() -> float:
        return round(time.monotonic() - started, 1)

    def finish(outcome: str) -> int:
        for leftover in out.glob("*.ppm.tmp"):
            leftover.unlink()
        result["outcome"] = outcome
        result["elapsed_seconds"] = elapsed()
        result["serial_bytes"] = serial_log.stat().st_size if serial_log.exists() else 0
        (host / "vm-result.json").write_text(json.dumps(result, indent=2) + "\n")
        print(f"[{elapsed():7.1f}s] outcome: {outcome}", flush=True)
        return 0 if outcome == "agent-reported" else 1

    def serial_tail(lines: int = 150) -> None:
        text = clean(serial_log.read_bytes().decode("utf-8", errors="replace")) if serial_log.exists() else ""
        tail = [line for line in text.split("\n") if line.strip()][-lines:]
        print(f"----- last {len(tail)} lines of the serial console -----")
        print("\n".join(tail))
        print("----- end of serial console -----", flush=True)

    try:
        qmp = Qmp(str(qmp_path), process)
    except RuntimeError as error:
        qemu_log.flush()
        print(f"QEMU did not start: {error}\n{(host / 'qemu.log').read_text()}")
        if process.poll() is None:
            process.kill()
        return finish("qemu-failed")

    kvm = qmp.command("query-kvm")
    result["kvm"] = kvm
    result["qemu_status"] = qmp.command("query-status")
    print(f"[{elapsed():7.1f}s] QEMU running, KVM: {kvm}", flush=True)
    if not kvm.get("enabled"):
        qmp.command("quit")
        return finish("kvm-not-enabled")

    # ---- watch the serial console ---------------------------------------------
    watcher = SerialWatcher(serial_log, elapsed)
    result["milestones"] = watcher.milestones
    result["screenshots"] = {}
    deadline = started + args.timeout

    def shoot(name: str, attempts: int, pause: float) -> None:
        """Screenshot a boot; retry while the display still looks unrendered."""
        tries = []
        for attempt in range(1, attempts + 1):
            try:
                shot = screenshot(qmp, work, out / f"screenshot-{name}.png")
            except Exception as error:
                shot = {"error": str(error), "looks_rendered": False}
            shot["at_seconds"] = elapsed()
            tries.append(shot)
            print(f"[{elapsed():7.1f}s] screenshot {name} #{attempt}: {shot}", flush=True)
            if shot["looks_rendered"] or attempt == attempts:
                break
            time.sleep(pause)
        result["screenshots"][name] = dict(tries[-1], attempts=len(tries), taken="after the agent reported")

    outcome = "timeout"
    first_boot_shot = False
    while time.monotonic() < deadline:
        if process.poll() is not None:
            outcome = "qemu-exited"
            break
        time.sleep(0.5)
        watcher.pump()
        qmp.drain()
        if watcher.reached("first_boot_done") and not first_boot_shot:
            # The guest reboots about 20 s after this marker: be quick.
            first_boot_shot = True
            shoot("first-boot", attempts=3, pause=4)
        if watcher.reached("agent_complete"):
            outcome = "agent-reported"
            break

    result["boots_seen"] = watcher.boot
    result["resets_seen"] = sum(1 for event in qmp.events if event["event"] == "RESET")
    result["events"] = [
        {"event": event["event"], "at_seconds": round(event["host_time"] - started, 1)} for event in qmp.events
    ]

    if outcome != "agent-reported":
        missing = [name for name, _pattern in MARKERS if not watcher.reached(name)]
        print(f"[{elapsed():7.1f}s] the guest did not report: {outcome}; markers not seen: {', '.join(missing)}")
        if process.poll() is None:
            try:
                shot = screenshot(qmp, work, out / "screenshot-at-failure.png")
                shot["taken"] = "at failure, for diagnosis"
                result["screenshots"]["at-failure"] = shot
                print(f"screenshot at failure: {shot}")
            except Exception as error:  # diagnosis only
                print(f"no screenshot at failure: {error}")
            try:
                qmp.command("quit")
            except Exception:
                process.kill()
        serial_tail()
        return finish(outcome)

    # ---- screenshot: the session has had its hold time; allow slow painting ----
    shoot("second-boot", attempts=9, pause=5)

    # The greeter shows a clock until someone touches the machine. Press Shift and
    # move the pointer, then look again: a display that responds is a live session,
    # not a frozen frame. Informational; never decides the result.
    try:
        qmp.command("send-key", keys=[{"type": "qcode", "data": "shift"}])
        qmp.command(
            "input-send-event",
            events=[
                {"type": "rel", "data": {"axis": "x", "value": 40}},
                {"type": "rel", "data": {"axis": "y", "value": 40}},
            ],
        )
        time.sleep(4)
        shot = screenshot(qmp, work, out / "screenshot-second-boot-after-input.png")
        shot["changed_fraction"] = changed_fraction(
            out / "screenshot-second-boot.ppm.tmp", out / "screenshot-second-boot-after-input.ppm.tmp"
        )
        shot["at_seconds"] = elapsed()
        shot["taken"] = "after a key press and pointer movement"
        result["screenshots"]["second-boot-after-input"] = shot
        print(f"[{elapsed():7.1f}s] screenshot after input: {shot}", flush=True)
    except Exception as error:
        print(f"[{elapsed():7.1f}s] no screenshot after input: {error}", flush=True)

    # ---- power off ----------------------------------------------------------------
    result["shutdown"] = "forced"
    try:
        asked = elapsed()
        qmp.command("system_powerdown")
        print(f"[{elapsed():7.1f}s] asked the guest to power off", flush=True)
        process.wait(timeout=180)
        result["shutdown"] = "clean"
        result["shutdown_seconds"] = round(elapsed() - asked, 1)
    except Exception as error:
        print(f"[{elapsed():7.1f}s] the guest did not power off: {error}; stopping QEMU")
        try:
            qmp.command("quit")
            process.wait(timeout=20)
        except Exception:
            process.kill()
    print(f"[{elapsed():7.1f}s] QEMU exited ({result['shutdown']})", flush=True)
    size = report_tar.stat().st_size if report_tar.exists() else 0
    result["report_tar_bytes"] = size
    print(f"guest report: {size} bytes; serial log: {serial_log.stat().st_size} bytes")
    return finish("agent-reported")


if __name__ == "__main__":
    sys.exit(main())
