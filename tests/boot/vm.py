#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""QEMU/KVM, QMP and serial-console plumbing shared by the boot tests.

Used by tests/boot/run-vm.py (the boot test) and tests/boot/rollback.py (the
boot-health scenarios). Nothing here decides whether a test passed.

- `Machine` builds the QEMU command line and owns the process. KVM is the only
  accelerator: without it QEMU fails, it never falls back to emulation
  (ADR-0002). The firmware-variable file, the serial log and the report channel
  survive a restart of QEMU, so a "power cut" can be followed by another start
  on the same disk.
- `Qmp` is a minimal QMP client that also collects asynchronous events
  (RESET, WATCHDOG, GUEST_PANICKED, SHUTDOWN) with the time they were seen.
- `SerialWatcher` follows the serial log. It numbers the boots by the kernel's
  banner and records the kernel command line of every boot: the kernel prints
  it before anything can fail, so it exists even for a boot that never reaches
  userspace.

Run in CI only: never on a shared or production machine.
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import shutil
import signal
import socket
import struct
import subprocess
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
KERNEL_CMDLINE = re.compile(r"Kernel command line: (.*)$")
# QMP events worth keeping with their payload.
EVENTS_WITH_DATA = ("WATCHDOG", "GUEST_PANICKED", "RESET", "SHUTDOWN", "STOP", "RESUME")


def clean(text: str) -> str:
    """Serial text without terminal control sequences."""
    text = ANSI.sub("", text).replace("\r\n", "\n").replace("\r", "\n")
    return "".join(ch for ch in text if ch in "\n\t" or ch >= " ")


def tur_arguments(cmdline: str) -> dict:
    """The boot-health arguments of a kernel command line (design section 5.4)."""
    words = cmdline.split()
    values = {}
    for word in words:
        key, sep, value = word.partition("=")
        if sep and key.startswith("tur."):
            values[key] = value
    tries = values.get("tur.boot_tries_left")
    ostree = next((word.split("=", 1)[1] for word in words if word.startswith("ostree=")), None)
    return {
        "contract": values.get("tur.boot_contract"),
        "boot": values.get("tur.boot"),
        "tries_left": int(tries) if tries is not None and re.fullmatch(r"-?\d+", tries) else tries,
        "has_dollar": "$" in cmdline,
        "ostree": ostree,
        "trial_arguments": [
            word
            for word in words
            if word.split("=", 1)[0] in ("panic", "rd.shell", "rd.emergency", "systemd.watchdog_sec")
        ],
    }


def label(arguments: dict) -> str:
    """A boot's kind as the design writes it: none, trial/N or fallback."""
    kind = arguments.get("boot")
    if kind == "trial":
        return f"trial/{arguments.get('tries_left')}"
    return kind or "none"


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

    def _event(self, reply: dict) -> None:
        event = {"event": reply["event"], "host_time": time.monotonic()}
        if reply["event"] in EVENTS_WITH_DATA and reply.get("data"):
            event["data"] = reply["data"]
        self.events.append(event)

    def command(self, name: str, **arguments):
        message: dict = {"execute": name}
        if arguments:
            message["arguments"] = arguments
        self.sock.sendall(json.dumps(message).encode() + b"\r\n")
        while True:
            reply = self._read()
            if "event" in reply:
                self._event(reply)
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
                self._event(reply)

    def close(self) -> None:
        try:
            self.sock.close()
        except OSError:
            pass


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
    """Follows the serial log.

    `boot` counts kernel banners. `boots` has one record per kernel start with
    the command line the kernel printed and the texts from `patterns` seen
    during that boot. `markers` are (name, regex) pairs matched against the
    test agent's marker lines; the first sighting of each per boot is a
    milestone named `boot<N>.<name>`.
    """

    def __init__(self, path: pathlib.Path, elapsed, markers=(), patterns=(), quiet: bool = False):
        self.path, self.elapsed = path, elapsed
        self.markers = list(markers)
        self.patterns = [(name, re.compile(regex)) for name, regex in patterns]
        self.offset, self.pending = 0, ""
        self.boot = 0
        self.boots: list[dict] = []
        self.milestones: dict[str, float] = {}
        self.printed: set[str] = set()
        self.agent_lines: list[dict] = []
        self.quiet = quiet
        self.before_first_kernel: dict[str, int] = {}

    def pump(self) -> list[str]:
        """Read what is new; return the new complete lines, cleaned."""
        if not self.path.exists():
            return []
        with open(self.path, "rb") as stream:
            stream.seek(self.offset)
            data = stream.read()
        self.offset += len(data)
        self.pending += data.decode("utf-8", errors="replace")
        complete, _, self.pending = self.pending.rpartition("\n")
        lines = [line for line in clean(complete).split("\n") if line.strip()]
        for line in lines:
            self.line(line)
        return lines

    def note(self, name: str) -> None:
        if name not in self.milestones:
            self.milestones[name] = self.elapsed()
            if not self.quiet:
                print(f"[{self.elapsed():7.1f}s] milestone: {name}", flush=True)

    def current(self) -> dict | None:
        return self.boots[-1] if self.boots else None

    def line(self, line: str) -> None:
        self.note("firmware_output")
        if KERNEL_BANNER.search(line):
            self.boot += 1
            self.boots.append(
                {"boot": self.boot, "kernel_started_at": self.elapsed(), "cmdline": None, "tur": None, "seen": {}}
            )
            self.note(f"boot{self.boot}.kernel_started")
        current = self.current()
        match = KERNEL_CMDLINE.search(line)
        if match and current is not None and current["cmdline"] is None:
            current["cmdline"] = match.group(1).strip()
            current["tur"] = tur_arguments(current["cmdline"])
            print(
                f"[{self.elapsed():7.1f}s] boot {self.boot}: tur.boot={label(current['tur'])} "
                f"contract={current['tur']['contract']} ostree={current['tur']['ostree']}",
                flush=True,
            )
        for name, pattern in self.patterns:
            if pattern.search(line):
                seen = current["seen"] if current is not None else self.before_first_kernel
                if name not in seen and not self.quiet:
                    print(f"[{self.elapsed():7.1f}s] boot {self.boot}: console shows {name}: {line.strip()[:160]}", flush=True)
                seen[name] = seen.get(name, 0) + 1
        if MARK in line:
            text = line[line.index(MARK):]
            if text not in self.printed:
                self.printed.add(text)
                self.agent_lines.append({"boot": self.boot, "at": self.elapsed(), "text": text[len(MARK):]})
                if not self.quiet:
                    print(f"[{self.elapsed():7.1f}s] guest: {text}", flush=True)
            for name, pattern in self.markers:
                if pattern.search(text):
                    self.note(f"boot{max(self.boot, 1)}.{name}")

    def reached(self, name: str) -> bool:
        return any(key.endswith("." + name) for key in self.milestones)

    def sequence(self) -> list[str]:
        """The kind of every boot seen, in order: none, trial/N, fallback."""
        return [label(boot["tur"]) if boot["tur"] else "unknown" for boot in self.boots]


def find_ovmf() -> tuple[str, str]:
    for code, variables in OVMF_CANDIDATES:
        if os.path.exists(code) and os.path.exists(variables):
            return code, variables
    raise SystemExit("no OVMF firmware found; install the ovmf package")


class Machine:
    """One virtual machine: a disk, firmware variables, a serial log.

    `start()` may be called again after the process has gone (a clean power
    off, or `kill()` standing in for a power cut). The firmware variables, the
    serial log and the report channel are kept and appended to.
    """

    def __init__(
        self,
        *,
        disk: str,
        work: pathlib.Path,
        serial_log: pathlib.Path,
        report_tar: pathlib.Path,
        qemu_log: pathlib.Path,
        name: str = "tur-boot-test",
        disk_format: str = "raw",
        disk_cache: str = "unsafe",
        cpus: int = 2,
        memory: int = 4096,
        qemu: str = "qemu-system-x86_64",
        images_disk: str | None = None,
        fw_cfg: dict[str, str] | None = None,
        watchdog: bool = False,
        pvpanic: bool = False,
        variables_from: str | None = None,
    ):
        self.disk, self.disk_format, self.disk_cache = disk, disk_format, disk_cache
        self.work, self.serial_log, self.report_tar, self.qemu_log = work, serial_log, report_tar, qemu_log
        self.name, self.cpus, self.memory, self.qemu = name, cpus, memory, qemu
        self.images_disk, self.fw_cfg = images_disk, dict(fw_cfg or {})
        self.watchdog, self.pvpanic = watchdog, pvpanic
        self.code, self.variables_template = find_ovmf()
        # Firmware variables of an earlier run of the same disk, so that the
        # boot entry shim created on the first boot is still there.
        if variables_from and os.path.exists(variables_from):
            self.variables_template = variables_from
        self.variables = work / f"{name}-OVMF_VARS.fd"
        self.qmp_path = work / f"{name}-qmp.sock"
        self.process: subprocess.Popen | None = None
        self.qmp: Qmp | None = None
        self.starts = 0
        self.events: list[dict] = []
        self.version = subprocess.run([qemu, "--version"], capture_output=True, text=True).stdout.splitlines()[0]

    def command(self) -> list[str]:
        command = [
            self.qemu,
            "-name", self.name,
            # KVM only: without it QEMU must fail, not fall back to emulation.
            "-machine", "q35,accel=kvm",
            "-cpu", "host",
            "-smp", str(self.cpus),
            "-m", str(self.memory),
            "-drive", f"if=pflash,format=raw,unit=0,readonly=on,file={self.code}",
            "-drive", f"if=pflash,format=raw,unit=1,file={self.variables}",
            "-drive", f"if=none,id=disk0,format={self.disk_format},cache={self.disk_cache},file={self.disk}",
            "-device", "virtio-blk-pci,drive=disk0,serial=tur-boot-test",
            "-netdev", "user,id=net0",
            "-device", "virtio-net-pci,netdev=net0",
            "-device", "virtio-rng-pci",
            # A paravirtual display with no host GPU and no 3D: the guest renders in software.
            "-device", "virtio-vga",
            "-display", "none",
            "-chardev", f"file,id=serial0,path={self.serial_log},append=on",
            "-serial", "chardev:serial0",
            "-device", "virtio-serial-pci",
            "-chardev", f"file,id=report,path={self.report_tar},append=on",
            "-device", "virtserialport,chardev=report,name=org.tur.boottest",
            "-qmp", f"unix:{self.qmp_path},server=on,wait=off",
            "-monitor", "none",
            "-parallel", "none",
        ]
        if self.images_disk:
            # The update source of the scenarios: an OCI layout on a read-only disk.
            command += [
                "-drive", f"if=none,id=images,format=raw,readonly=on,file={self.images_disk}",
                "-device", "virtio-blk-pci,drive=images,serial=tur-test-images",
            ]
        for key, value in self.fw_cfg.items():
            command += ["-fw_cfg", f"name=opt/org.tur/{key},string={value}"]
        if self.watchdog:
            # An emulated hardware watchdog. Its expiry resets the machine and is
            # reported as a QMP WATCHDOG event (design section 9).
            command += ["-device", "i6300esb", "-action", "watchdog=reset"]
        if self.pvpanic:
            # Reports a guest panic as a QMP event once the guest has loaded the
            # driver. The default action for a panic is to stop the machine;
            # the test wants the guest's own panic= timeout to act.
            command += ["-device", "pvpanic-pci", "-action", "panic=none"]
        return command

    def start(self) -> Qmp:
        if self.process is not None and self.process.poll() is None:
            raise RuntimeError("the machine is already running")
        if not self.variables.exists():
            shutil.copyfile(self.variables_template, self.variables)
        self.qmp_path.unlink(missing_ok=True)
        log = open(self.qemu_log, "a")
        log.write(f"\n===== start {self.starts + 1}: {' '.join(self.command())}\n")
        log.flush()
        self.process = subprocess.Popen(self.command(), stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
        log.close()
        self.starts += 1
        self.qmp = Qmp(str(self.qmp_path), self.process)
        return self.qmp

    def alive(self) -> bool:
        return self.process is not None and self.process.poll() is None

    def collect(self) -> None:
        """Move the QMP events seen so far into `events`."""
        if self.qmp is None:
            return
        try:
            self.qmp.drain()
        except OSError:
            pass
        self.events.extend(self.qmp.events)
        self.qmp.events = []

    def kill(self) -> None:
        """Stop QEMU at once, as a power cut would stop the machine."""
        self.collect()
        if self.process is not None and self.process.poll() is None:
            self.process.send_signal(signal.SIGKILL)
            self.process.wait(timeout=30)
        if self.qmp is not None:
            self.qmp.close()
            self.qmp = None

    def quit(self) -> None:
        """Stop QEMU without asking the guest."""
        self.collect()
        if self.process is not None and self.process.poll() is None:
            try:
                if self.qmp is not None:
                    self.qmp.command("quit")
                self.process.wait(timeout=20)
            except Exception:
                self.process.kill()
                self.process.wait(timeout=30)
        if self.qmp is not None:
            self.qmp.close()
            self.qmp = None

    def powerdown(self, timeout: float = 180) -> bool:
        """Ask the guest to power off through ACPI; True if it did."""
        if not self.alive() or self.qmp is None:
            return False
        try:
            self.qmp.command("system_powerdown")
            self.process.wait(timeout=timeout)
        except Exception:
            self.quit()
            return False
        self.collect()
        self.qmp.close()
        self.qmp = None
        return True
