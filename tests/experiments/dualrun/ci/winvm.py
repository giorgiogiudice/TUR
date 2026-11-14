# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""One virtual machine of the experiment: QEMU, its TPM, its control socket, the agent.

Used for both configurations: the stand-in PC ("native") and TUR's guest.
The command line comes from dualrunlib.qemu; this module adds what a test
needs around it: a software TPM, the fallback over Hyper-V enlightenment
sets, screenshots with their text read back (so that a hung start says what
is on the screen), pointer and key input, and the line protocol of TUR's
test agent on the serial port.
"""

from __future__ import annotations

import base64
import json
import os
import pathlib
import queue
import signal
import socket
import subprocess
import sys
import threading
import time

HERE = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HERE))

from dualrunlib import qemu  # noqa: E402
from dualrunlib.qmp import Qmp, QmpError  # noqa: E402

PREFIX = "@@DR@@"


class AgentError(Exception):
    """The agent did not answer, or the script it ran failed."""


class Agent:
    def __init__(self, path: str, log: pathlib.Path):
        self.path = path
        self.log = open(log, "ab")
        self.messages: "queue.Queue[dict]" = queue.Queue()
        self.sock: socket.socket | None = None
        self.next_id = 1
        self.hello: dict | None = None
        self.closed = False
        self.thread = threading.Thread(target=self._reader, daemon=True)

    def connect(self, timeout: float = 30) -> None:
        deadline = time.time() + timeout
        while True:
            try:
                self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                self.sock.connect(self.path)
                break
            except OSError:
                self.sock.close()
                if time.time() > deadline:
                    raise AgentError(f"cannot connect to the serial socket {self.path}")
                time.sleep(0.2)
        self.thread.start()

    def _reader(self) -> None:
        buffer = b""
        while not self.closed:
            try:
                chunk = self.sock.recv(65536)
            except OSError:
                break
            if not chunk:
                break
            buffer += chunk
            while b"\n" in buffer:
                line, buffer = buffer.split(b"\n", 1)
                text = line.decode("latin-1").strip("\r")
                at = text.find(PREFIX)
                if at < 0:
                    self.log.write(line + b"\n")
                    self.log.flush()
                    continue
                try:
                    self.messages.put(json.loads(text[at + len(PREFIX):]))
                except ValueError:
                    self.log.write(b"[unparsed] " + line[:300] + b"\n")
            if len(buffer) > 16 * 1024 * 1024:
                buffer = b""

    def wait_hello(self, timeout: float, alive=lambda: True) -> dict | None:
        """The agent's first line after it started, or None."""
        deadline = time.time() + timeout
        while time.time() < deadline and alive():
            try:
                message = self.messages.get(timeout=1)
            except queue.Empty:
                continue
            if message.get("ev") == "hello":
                self.hello = message
                return message
        return None

    def ps(self, script: str, timeout: float = 180) -> str:
        """Run a PowerShell script in the guest and return what it printed."""
        if self.sock is None:
            raise AgentError("not connected")
        number = self.next_id
        self.next_id += 1
        request = {"id": number, "b64": base64.b64encode(script.encode()).decode()}
        try:
            self.sock.sendall((PREFIX + json.dumps(request) + "\n").encode())
        except OSError as error:
            raise AgentError(f"cannot send: {error}")
        deadline = time.time() + timeout
        while time.time() < deadline:
            try:
                message = self.messages.get(timeout=1)
            except queue.Empty:
                continue
            if message.get("id") != number:
                continue
            text = base64.b64decode(message.get("out") or "").decode("utf-8", "replace")
            if not message.get("ok"):
                raise AgentError(f"the script failed: {text.strip()[:600]}")
            return text
        raise AgentError(f"no answer within {timeout:.0f} s")

    def json(self, script: str, timeout: float = 180):
        """Run a script whose last line is JSON."""
        text = self.ps(script, timeout)
        for line in reversed(text.strip().splitlines()):
            line = line.strip()
            if line.startswith("{") or line.startswith("["):
                return json.loads(line)
        raise AgentError(f"no JSON in the answer: {text.strip()[:300]}")

    def close(self) -> None:
        self.closed = True
        try:
            if self.sock:
                self.sock.close()
        except OSError:
            pass


class Machine:
    def __init__(self, name: str, cfg: dict, out: pathlib.Path, run: pathlib.Path, tpm_dir: pathlib.Path | None = None):
        self.name = name
        self.cfg = dict(cfg)
        self.out = out
        self.run = run
        self.tpm_dir = tpm_dir
        out.mkdir(parents=True, exist_ok=True)
        run.mkdir(parents=True, exist_ok=True)
        self.cfg["name"] = name
        self.cfg["qmp"] = str(run / "qmp.sock")
        self.cfg["serial"] = str(run / "serial.sock")
        self.process: subprocess.Popen | None = None
        self.swtpm: subprocess.Popen | None = None
        self.qmp: Qmp | None = None
        self.agent = Agent(self.cfg["serial"], out / "serial.log")
        self.started = 0.0
        self.shots = 0
        self.hyperv_used: list[str] | None = None

    # -- life cycle -------------------------------------------------------

    def _start_tpm(self) -> None:
        if self.tpm_dir is None:
            return
        self.tpm_dir.mkdir(parents=True, exist_ok=True)
        sock = self.run / "swtpm.sock"
        sock.unlink(missing_ok=True)
        self.swtpm = subprocess.Popen(
            ["swtpm", "socket", "--tpm2", "--tpmstate", f"dir={self.tpm_dir}", "--ctrl", f"type=unixio,path={sock}",
             "--terminate", "--log", f"file={self.out / 'swtpm.log'},level=1"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.time() + 10
        while not sock.exists() and time.time() < deadline:
            time.sleep(0.1)
        self.cfg["tpm_socket"] = str(sock)

    def start(self) -> None:
        sets = qemu.HYPERV_SETS if self.cfg.get("hyperv_auto") else [self.cfg.get("hyperv", [])]
        last_error = ""
        for flags in sets:
            for stale in ("qmp.sock", "serial.sock"):
                (self.run / stale).unlink(missing_ok=True)
            self._start_tpm()
            cfg = dict(self.cfg, hyperv=flags)
            if not flags:
                cfg["balloon"] = False          # Hyper-V's memory device needs its bus, which needs these
            arguments = qemu.build(cfg)
            (self.out / "qemu-command.txt").write_text(" ".join(arguments) + "\n")
            log = open(self.out / "qemu.log", "ab")
            self.process = subprocess.Popen(arguments, stdout=log, stderr=log, start_new_session=True)
            self.started = time.time()
            time.sleep(3)
            if self.process.poll() is None:
                self.hyperv_used = flags
                self.cfg = cfg
                break
            last_error = (self.out / "qemu.log").read_text(errors="replace")[-600:]
            self._stop_tpm()
        else:
            raise RuntimeError(f"QEMU did not start: {last_error}")
        self.qmp = Qmp(self.cfg["qmp"], timeout=60)
        self.agent.connect()

    def alive(self) -> bool:
        return self.process is not None and self.process.poll() is None

    def wait_exit(self, timeout: float) -> bool:
        deadline = time.time() + timeout
        while time.time() < deadline:
            if not self.alive():
                self._after_exit()
                return True
            time.sleep(0.5)
        return False

    def kill(self) -> None:
        """A power cut for this machine."""
        if self.process is not None and self.process.poll() is None:
            self.process.send_signal(signal.SIGKILL)
            self.process.wait()
        self._after_exit()

    def _stop_tpm(self) -> None:
        if self.swtpm is not None:
            try:
                self.swtpm.terminate()
                self.swtpm.wait(timeout=10)
            except (OSError, subprocess.TimeoutExpired):
                self.swtpm.kill()
            self.swtpm = None

    def _after_exit(self) -> None:
        if self.qmp is not None:
            self.qmp.close()
            self.qmp = None
        self.agent.close()
        self._stop_tpm()

    # -- looking and acting ------------------------------------------------

    def screenshot(self, tag: str, keep: bool = True) -> tuple[pathlib.Path | None, str]:
        """A picture of the screen and the text read from it."""
        if self.qmp is None:
            return None, ""
        self.shots += 1
        path = self.out / f"screen-{self.shots:03d}-{tag}.png"
        try:
            self.qmp.command("screendump", filename=str(path), format="png")
        except QmpError:
            try:
                raw = path.with_suffix(".ppm")
                self.qmp.command("screendump", filename=str(raw))
                from PIL import Image
                Image.open(raw).save(path)
                raw.unlink()
            except Exception:  # noqa: BLE001
                return None, ""
        text = ""
        try:
            done = subprocess.run(["tesseract", str(path), "stdout", "--psm", "6"], capture_output=True,
                                  text=True, timeout=60)
            text = " ".join(done.stdout.split())[:700]
        except (OSError, subprocess.TimeoutExpired):
            pass
        if not keep:
            path.unlink(missing_ok=True)
            return None, text
        return path, text

    def key(self, name: str) -> None:
        if self.qmp is not None:
            try:
                self.qmp.command("send-key", keys=[{"type": "qcode", "data": name}])
            except QmpError:
                pass

    def click(self, x: int, y: int, width: int, height: int, button: str = "left") -> None:
        """Move the absolute pointer to a pixel of the guest's screen and click."""
        events = [
            {"type": "abs", "data": {"axis": "x", "value": int(x * 32767 / max(1, width - 1))}},
            {"type": "abs", "data": {"axis": "y", "value": int(y * 32767 / max(1, height - 1))}},
        ]
        self.qmp.command("input-send-event", events=events)
        time.sleep(0.15)
        self.qmp.command("input-send-event", events=[{"type": "btn", "data": {"down": True, "button": button}}])
        time.sleep(0.08)
        self.qmp.command("input-send-event", events=[{"type": "btn", "data": {"down": False, "button": button}}])

    def rss_mb(self) -> float | None:
        try:
            for line in pathlib.Path(f"/proc/{self.process.pid}/status").read_text().splitlines():
                if line.startswith("VmRSS:"):
                    return round(int(line.split()[1]) / 1024, 1)
        except (OSError, AttributeError):
            pass
        return None

    def seconds(self) -> float:
        return round(time.time() - self.started, 1)
