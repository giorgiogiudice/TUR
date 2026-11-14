# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The test session of the re-hosting experiment: a private session bus, a
headless Wayland compositor with Xwayland, and the accessibility bus.

The compositor is sway (wlroots, headless backend, software renderer), not
KWin: it is small, and its window list says for every window whether it is
a Wayland window or an X11 one through Xwayland. The report says so where
it matters. Screenshots are taken with grim.

Also here: a minimal Chrome DevTools Protocol client (standard library
only), used to ask a Chromium-based application about its own page.
"""

import base64
import glob
import json
import os
import socket
import struct
import subprocess
import time
import urllib.request


def run(command, env=None, timeout=30):
    try:
        done = subprocess.run(command, env=env, capture_output=True, text=True, errors="replace", timeout=timeout)
        return done.returncode, done.stdout, done.stderr
    except (subprocess.TimeoutExpired, FileNotFoundError) as problem:
        return 124, "", str(problem)


class Session:
    def __init__(self, work, logs):
        self.runtime = os.path.join(work, "run")
        os.makedirs(self.runtime, exist_ok=True)
        os.chmod(self.runtime, 0o700)
        self.logs = logs
        os.makedirs(logs, exist_ok=True)
        self.processes = []
        self.display = None
        self.a11y = {"how": "not started"}
        self.base = {
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "LANG": "C.UTF-8",
            "LC_ALL": "C.UTF-8",
            "XDG_RUNTIME_DIR": self.runtime,
            "DBUS_SESSION_BUS_ADDRESS": f"unix:path={self.runtime}/bus",
            "XDG_SESSION_TYPE": "wayland",
            "XDG_CURRENT_DESKTOP": "sway",
            "LIBGL_ALWAYS_SOFTWARE": "1",
            "SDL_AUDIODRIVER": "dummy",  # the runner has no sound card
            "HOME": os.path.join(work, "session-home"),
        }
        os.makedirs(self.base["HOME"], exist_ok=True)

    def start(self, name, command, env):
        log = open(os.path.join(self.logs, name + ".log"), "wb")
        process = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
        self.processes.append(process)
        return process

    def env(self, **extra):
        environment = dict(self.base)
        if self.display:
            environment["DISPLAY"] = self.display
        if "AT_SPI_BUS_ADDRESS" in self.a11y:
            environment["AT_SPI_BUS_ADDRESS"] = self.a11y["AT_SPI_BUS_ADDRESS"]
        environment.update(extra)
        return environment

    def wait(self, test, seconds, step=0.25):
        deadline = time.time() + seconds
        while time.time() < deadline:
            if test():
                return True
            time.sleep(step)
        return False

    def up(self):
        bus = os.path.join(self.runtime, "bus")
        self.start("dbus", ["dbus-daemon", "--session", "--nofork", "--nopidfile", f"--address=unix:path={bus}"], self.base)
        if not self.wait(lambda: os.path.exists(bus), 10):
            raise RuntimeError("the session bus did not start")
        config = os.path.join(self.runtime, "sway.conf")
        with open(config, "w", encoding="utf-8") as handle:
            handle.write("xwayland enable\noutput HEADLESS-1 resolution 1280x800 position 0 0 bg #20303c solid_color\ndefault_border none\nfocus_follows_mouse no\n")
        sway_env = dict(self.base, WLR_BACKENDS="headless", WLR_LIBINPUT_NO_DEVICES="1", WLR_RENDERER="pixman")
        self.start("sway", ["sway", "-c", config], sway_env)
        if not self.wait(lambda: glob.glob(os.path.join(self.runtime, "sway-ipc.*.sock")) and glob.glob(os.path.join(self.runtime, "wayland-*")), 30):
            raise RuntimeError("sway did not start: " + open(os.path.join(self.logs, "sway.log"), errors="replace").read()[-800:])
        self.base["SWAYSOCK"] = glob.glob(os.path.join(self.runtime, "sway-ipc.*.sock"))[0]
        sockets = [path for path in glob.glob(os.path.join(self.runtime, "wayland-*")) if not path.endswith(".lock")]
        self.base["WAYLAND_DISPLAY"] = os.path.basename(sorted(sockets)[0])
        marker = os.path.join(self.runtime, "display")
        run(["swaymsg", "exec", f"sh -c 'echo $DISPLAY > {marker}'"], env=self.base)
        self.wait(lambda: os.path.exists(marker) and os.path.getsize(marker) > 0, 10)
        self.display = open(marker).read().strip() if os.path.exists(marker) else None
        self.accessibility_up()

    def accessibility_up(self):
        """As tests/pax/a11y/harness.py does: the launcher by activation,
        else the two daemons by hand; then accessibility switched on, as a
        session with an assistive technology has it."""
        environment = self.env()
        code, out, err = run(["gdbus", "call", "--session", "--dest", "org.a11y.Bus", "--object-path", "/org/a11y/bus", "--method", "org.a11y.Bus.GetAddress"], env=environment)
        address = out.split("'")[1] if code == 0 and "'" in out else ""
        if address:
            self.a11y = {"how": "at-spi-bus-launcher, activated on the session bus", "address": address}
        else:
            a11y_bus = os.path.join(self.runtime, "a11y-bus")
            self.start("a11y-bus", ["dbus-daemon", "--config-file=/usr/share/defaults/at-spi2/accessibility.conf", "--nofork", "--nopidfile", f"--address=unix:path={a11y_bus}"], environment)
            self.wait(lambda: os.path.exists(a11y_bus), 10)
            address = f"unix:path={a11y_bus}"
            self.a11y = {"how": f"daemons started by hand (the launcher said: {(err or out)[:200]})", "address": address, "AT_SPI_BUS_ADDRESS": address}
            registryd = next((path for path in ("/usr/libexec/at-spi2-registryd", "/usr/lib/at-spi2-core/at-spi2-registryd") if os.path.exists(path)), "/usr/libexec/at-spi2-registryd")
            self.start("registryd", [registryd], self.env())
            time.sleep(2)
        for name in ("IsEnabled", "ScreenReaderEnabled"):
            code, out, err = run(["gdbus", "call", "--session", "--dest", "org.a11y.Bus", "--object-path", "/org/a11y/bus", "--method", "org.freedesktop.DBus.Properties.Set", "org.a11y.Status", name, "<true>"], env=self.env())
            self.a11y[name] = code == 0

    def windows(self):
        """Every window sway knows: title, size, and whether it is a
        Wayland window (xdg_shell) or an X11 one (xwayland)."""
        code, out, _ = run(["swaymsg", "-t", "get_tree", "-r"], env=self.base, timeout=10)
        if code != 0:
            return []
        found = []

        def visit(node):
            if node.get("pid") and node.get("type") in ("con", "floating_con") and (node.get("app_id") is not None or node.get("window") is not None):
                rect = node.get("rect") or {}
                found.append(
                    {
                        "title": (node.get("name") or "")[:120],
                        "app_id": node.get("app_id") or (node.get("window_properties") or {}).get("class"),
                        "shell": node.get("shell") or ("xwayland" if node.get("window") else "xdg_shell"),
                        "pid": node.get("pid"),
                        "width": rect.get("width", 0),
                        "height": rect.get("height", 0),
                    }
                )
            for child in (node.get("nodes") or []) + (node.get("floating_nodes") or []):
                visit(child)

        try:
            visit(json.loads(out))
        except ValueError:
            return []
        return found

    def close_windows(self):
        run(["swaymsg", "[title=.*] kill"], env=self.base, timeout=10)

    def screenshot(self, path):
        code, _, err = run(["grim", path], env=self.base, timeout=20)
        return code == 0 and os.path.exists(path)

    def down(self):
        for process in self.processes[::-1]:
            try:
                process.terminate()
                process.wait(timeout=5)
            except (subprocess.TimeoutExpired, OSError):
                try:
                    process.kill()
                except OSError:
                    pass


# ------------------------------------------------- Chrome DevTools Protocol


class DevTools:
    def __init__(self, port):
        self.port = port
        self.counter = 0

    def targets(self):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{self.port}/json/list", timeout=3) as response:
                return json.loads(response.read())
        except (OSError, ValueError):
            return None

    def version(self):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{self.port}/json/version", timeout=3) as response:
                return json.loads(response.read())
        except (OSError, ValueError):
            return None

    def call(self, websocket_url, method, params=None, timeout=10):
        """One command on a new connection; returns the result or None."""
        try:
            rest = websocket_url.split("://", 1)[1]
            host, path = rest.split("/", 1)
            name, port = host.split(":")
            connection = socket.create_connection((name, int(port)), timeout=timeout)
        except (OSError, ValueError):
            return None
        try:
            key = base64.b64encode(os.urandom(16)).decode()
            connection.sendall((f"GET /{path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n").encode())
            buffer = b""
            while b"\r\n\r\n" not in buffer:
                chunk = connection.recv(4096)
                if not chunk:
                    return None
                buffer += chunk
            head, buffer = buffer.split(b"\r\n\r\n", 1)
            if b" 101 " not in head.split(b"\r\n", 1)[0]:
                return None
            self.counter += 1
            payload = json.dumps({"id": self.counter, "method": method, "params": params or {}}).encode()
            mask = os.urandom(4)
            header = b"\x81"
            if len(payload) < 126:
                header += bytes([0x80 | len(payload)])
            elif len(payload) < 65536:
                header += bytes([0x80 | 126]) + struct.pack(">H", len(payload))
            else:
                header += bytes([0x80 | 127]) + struct.pack(">Q", len(payload))
            connection.sendall(header + mask + bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload)))
            message = b""
            deadline = time.time() + timeout
            while time.time() < deadline:
                while len(buffer) < 2:
                    chunk = connection.recv(65536)
                    if not chunk:
                        return None
                    buffer += chunk
                opcode = buffer[0] & 0x0F
                final = buffer[0] & 0x80
                length = buffer[1] & 0x7F
                offset = 2
                if length == 126:
                    while len(buffer) < 4:
                        buffer += connection.recv(65536)
                    length = struct.unpack(">H", buffer[2:4])[0]
                    offset = 4
                elif length == 127:
                    while len(buffer) < 10:
                        buffer += connection.recv(65536)
                    length = struct.unpack(">Q", buffer[2:10])[0]
                    offset = 10
                while len(buffer) < offset + length:
                    chunk = connection.recv(1 << 20)
                    if not chunk:
                        return None
                    buffer += chunk
                frame = buffer[offset : offset + length]
                buffer = buffer[offset + length :]
                if opcode == 8:
                    return None
                if opcode in (1, 2, 0):
                    message += frame
                    if final:
                        try:
                            reply = json.loads(message)
                        except ValueError:
                            reply = {}
                        message = b""
                        if reply.get("id") == self.counter:
                            return reply.get("result", reply.get("error"))
            return None
        except (OSError, ValueError, struct.error):
            return None
        finally:
            connection.close()

    def evaluate(self, websocket_url, expression, timeout=10):
        result = self.call(websocket_url, "Runtime.evaluate", {"expression": expression, "returnByValue": True}, timeout)
        if not result or "result" not in result:
            return None
        return result["result"].get("value")
