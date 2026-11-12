#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Installs, runs and removes real Windows applications through tur-runtime.

Run by .github/workflows/windows-runtime.yml on a hosted Ubuntu runner. It is
never run on a development machine (AGENTS.md).

    apps.py --tur-runtime BINARY --apps apps.json --work-dir DIR --evidence DIR

For every application in apps.json, in this order:

  1. download the installer from the publisher's own location and refuse it
     unless its size and SHA-256 are the pinned ones;
  2. `tur-runtime installer identify`: the kind, from the file's content;
  3. `tur-runtime app install`: a fresh prefix, the installer unattended,
     inside the sandbox; then the record, the launcher entry and the icon
     are looked at;
  4. start the application from the command line of its generated launcher
     entry, on a virtual display, and wait for a window of its own; take a
     screenshot; close it;
  5. one check that the program does its job (see CHECKS);
  6. `tur-runtime app uninstall`, then look for anything left.

What a result means. "pass" for an application says that these steps worked
for this version, with the distribution's Wine on this runner, without a
GPU. It is a measurement, not a statement of compatibility: one window and
one function were looked at. A failure is a result too. It is written into
the table as it happened; it is not retried and not hidden.

The exit status is 1 when a result differs from the one apps.json expects,
so that a regression, or an improvement nobody wrote down, is noticed.

For the comparison of case-insensitive prefixes (issue #81) the same
applications are installed again with options added to every `app install`
(--install-option --casefold, or --no-casefold), and --expect-casefold says
whether each prefix's drive_c must then carry the F attribute, as `lsattr`
reads it. --downloads shares the downloaded installers between passes; each
is checked against its pinned hash every time it is used.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import pathlib
import re
import shlex
import shutil
import socket
import struct
import subprocess
import sys
import threading
import time
import traceback
import urllib.request
import wave
import zipfile
import zlib

DISPLAY = ":99"
STAGES = ("install", "launch", "check", "uninstall")
PNG_MAGIC = b"\x89PNG\r\n\x1a\n"


def log(text: str) -> None:
    print(text, flush=True)


def run(command: list[str], env: dict[str, str] | None = None, timeout: int = 600) -> subprocess.CompletedProcess:
    """Runs a command, captures its output as text, never raises on failure."""
    try:
        return subprocess.run(command, env=env, capture_output=True, text=True, errors="replace",
                              timeout=timeout)
    except subprocess.TimeoutExpired as error:
        out = error.stdout.decode(errors="replace") if isinstance(error.stdout, bytes) else (error.stdout or "")
        err = error.stderr.decode(errors="replace") if isinstance(error.stderr, bytes) else (error.stderr or "")
        return subprocess.CompletedProcess(command, 124, out, err + f"\n[timed out after {timeout}s]")
    except OSError as error:
        return subprocess.CompletedProcess(command, 127, "", str(error))


def tail(text: str, lines: int = 12) -> str:
    return "\n".join(text.strip().splitlines()[-lines:])


def sha256_of(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def windows_path(path: pathlib.Path) -> str:
    """A path of this system as the runtime's Z: drive shows it."""
    return "Z:" + str(path).replace("/", "\\")


class World:
    """The scratch world the applications are installed into."""

    def __init__(self, tur: str, work: pathlib.Path, evidence: pathlib.Path,
                 downloads: pathlib.Path | None = None, install_options: list[str] | None = None,
                 expect_casefold: str | None = None, title: str | None = None) -> None:
        self.tur = str(pathlib.Path(tur).resolve())
        self.work = work
        self.evidence = evidence
        self.install_options = install_options or []
        self.expect_casefold = expect_casefold
        self.title = title or "Real applications through tur-runtime"
        self.data = work / "xdg-data"
        self.state = work / "xdg-state"
        self.shared = work / "shared"
        self.downloads = downloads or work / "downloads"
        self.profiles = work / "profiles"
        for directory in (self.data, self.state, self.shared, self.downloads, self.profiles, evidence):
            directory.mkdir(parents=True, exist_ok=True)

    def env(self, display: bool = True) -> dict[str, str]:
        env = dict(os.environ)
        env.pop("WAYLAND_DISPLAY", None)
        env.pop("DISPLAY", None)
        if display:
            env["DISPLAY"] = DISPLAY
        env["XDG_DATA_HOME"] = str(self.data)
        env["XDG_STATE_HOME"] = str(self.state)
        return env

    def tur_run(self, arguments: list[str], timeout: int = 1200) -> subprocess.CompletedProcess:
        done = run([self.tur, *arguments], env=self.env(), timeout=timeout)
        log(f"$ tur-runtime {' '.join(arguments)}\n  -> exit status {done.returncode}")
        if done.returncode != 0 and done.stderr.strip():
            log("  stderr: " + tail(done.stderr).replace("\n", "\n          "))
        return done

    def tur_json(self, arguments: list[str], timeout: int = 1200) -> tuple[int, dict, str]:
        done = self.tur_run(arguments, timeout=timeout)
        try:
            document = json.loads(done.stdout)
        except json.JSONDecodeError:
            document = {}
        return done.returncode, document, done.stderr

    def runtime_paths(self, app_id: str) -> list[pathlib.Path]:
        data = self.data / "tur/runtime"
        return [data / "prefixes" / app_id, data / "launchers" / app_id, data / "staging" / app_id,
                data / "backups" / app_id, self.state / "tur/runtime/logs" / app_id]


# ------------------------------------------------------------------ the display


class Window:
    def __init__(self, identifier: str, title: str, instance: str, klass: str, width: int, height: int) -> None:
        self.id = identifier
        self.title = title
        self.instance = instance
        self.klass = klass
        self.width = width
        self.height = height

    def describe(self) -> dict:
        return {"id": self.id, "title": self.title, "class": [self.instance, self.klass],
                "width": self.width, "height": self.height}


WINDOW_LINE = re.compile(r'^\s*(0x[0-9a-f]+) "(.*)": \("(.*?)" "(.*?)"\)\s+(\d+)x(\d+)')


def list_windows() -> list[Window]:
    """Every named window with a class on the virtual display."""
    tree = run(["xwininfo", "-display", DISPLAY, "-root", "-tree"], timeout=20).stdout
    windows = []
    for line in tree.splitlines():
        match = WINDOW_LINE.match(line)
        if match:
            identifier, title, instance, klass, width, height = match.groups()
            windows.append(Window(identifier, title, instance, klass, int(width), int(height)))
    return windows


def is_viewable(window: Window) -> bool:
    info = run(["xwininfo", "-display", DISPLAY, "-id", window.id], timeout=20).stdout
    return "Map State: IsViewable" in info


def start_display(evidence: pathlib.Path) -> list[subprocess.Popen]:
    """An X server without a screen, and a window manager, as a desktop has one."""
    server = subprocess.Popen(["Xvfb", DISPLAY, "-screen", "0", "1280x800x24", "-nolisten", "tcp", "-ac"],
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    number = DISPLAY.lstrip(":")
    for _ in range(100):
        if pathlib.Path(f"/tmp/.X11-unix/X{number}").exists():
            break
        time.sleep(0.1)
    env = dict(os.environ, DISPLAY=DISPLAY)
    manager = subprocess.Popen(["openbox"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(100):
        if run(["wmctrl", "-m"], env=env, timeout=10).returncode == 0:
            break
        time.sleep(0.1)
    described = run(["wmctrl", "-m"], env=env, timeout=10).stdout
    (evidence / "display.txt").write_text(
        f"Xvfb {DISPLAY}, 1280x800, 24-bit; no GPU: everything is drawn in software\n{described}")
    return [manager, server]


def screenshot(path: pathlib.Path) -> bool:
    done = run(["import", "-display", DISPLAY, "-window", "root", str(path)], timeout=60)
    if done.returncode != 0 or not path.is_file():
        log("  screenshot failed: " + tail(done.stderr, 3))
        return False
    # A small copy for the report: half the width, a reduced palette.
    small = path.with_name(path.stem + "-small.png")
    run(["convert", str(path), "-resize", "800x", "-colors", "128", "-strip", str(small)], timeout=60)
    return True


def close_window(window: Window) -> None:
    run(["wmctrl", "-i", "-c", window.id], env=dict(os.environ, DISPLAY=DISPLAY), timeout=20)


# -------------------------------------------------------------- launcher entries


def read_desktop_entry(path: pathlib.Path) -> dict[str, str]:
    """A desktop entry's keys, with the string escapes undone."""
    lines = path.read_text().splitlines()
    if not lines or lines[0] != "[Desktop Entry]":
        raise ValueError("not a desktop entry")
    entry = {}
    escapes = {"s": " ", "n": "\n", "t": "\t", "r": "\r", "\\": "\\"}
    for line in lines[1:]:
        key, _, value = line.partition("=")
        plain, index = "", 0
        while index < len(value):
            if value[index] == "\\" and index + 1 < len(value):
                plain += escapes.get(value[index + 1], value[index + 1])
                index += 2
            else:
                plain += value[index]
                index += 1
        entry[key] = plain
    return entry


def exec_command(entry: dict[str, str]) -> list[str]:
    """The command an Exec key stands for: quoted by the specification's
    rules, with a literal percent sign written twice."""
    return [argument.replace("%%", "%") for argument in shlex.split(entry["Exec"])]


def check_png(path: pathlib.Path) -> dict:
    """Reads a PNG the hard way: every chunk's CRC, and the image data
    decompressed to exactly the size the header promises."""
    data = path.read_bytes()
    if not data.startswith(PNG_MAGIC):
        raise ValueError("not a PNG")
    at, width, height, depth, colour, idat = 8, 0, 0, 0, 0, b""
    while at < len(data):
        length, kind = struct.unpack(">I4s", data[at:at + 8])
        body = data[at + 8:at + 8 + length]
        (crc,) = struct.unpack(">I", data[at + 8 + length:at + 12 + length])
        if zlib.crc32(kind + body) != crc:
            raise ValueError(f"chunk {kind!r} has a bad CRC")
        if kind == b"IHDR":
            width, height, depth, colour = struct.unpack(">IIBB", body[:10])
        elif kind == b"IDAT":
            idat += body
        at += 12 + length
    raw = zlib.decompress(idat)
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[colour]
    expected = height * (1 + (width * channels * depth + 7) // 8)
    if len(raw) != expected:
        raise ValueError(f"image data is {len(raw)} bytes, the header promises {expected}")
    return {"width": width, "height": height, "bit_depth": depth, "colour_type": colour, "bytes": len(data)}


# -------------------------------------------------------------------- one launch


class Launch:
    """An application started from its launcher entry's command line."""

    def __init__(self, world: World, command: list[str], arguments: list[str], log_path: pathlib.Path) -> None:
        self.before = {window.id for window in list_windows()}
        self.log_path = log_path
        self.output = log_path.open("w")
        full = command + (["--", *arguments] if arguments else [])
        log("$ " + " ".join(shlex.quote(part) for part in full))
        self.started = time.monotonic()
        self.process = subprocess.Popen(full, env=world.env(), stdout=self.output, stderr=subprocess.STDOUT,
                                        stdin=subprocess.DEVNULL)

    def new_windows(self) -> list[Window]:
        return [window for window in list_windows() if window.id not in self.before]

    def wait_for_window(self, klass: str, title: str | None, timeout: float) -> tuple[Window | None, float]:
        """The first viewable window of the application's own class, of a
        size a person could see, and with the wanted title when one is given."""
        wanted = klass.lower()
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            for window in self.new_windows():
                same_class = wanted in (window.instance.lower(), window.klass.lower())
                big_enough = window.width >= 120 and window.height >= 60
                titled = title is None or re.search(title, window.title) is not None
                if same_class and big_enough and titled and is_viewable(window):
                    return window, time.monotonic() - self.started
            if self.process.poll() is not None:
                break
            time.sleep(0.25)
        return None, time.monotonic() - self.started

    def close(self, klass: str) -> str:
        """Asks the application's windows to close, the way a person would,
        and says how it ended: "closed" or "killed"."""
        wanted = klass.lower()
        for _ in range(3):
            for window in self.new_windows():
                if wanted in (window.instance.lower(), window.klass.lower()):
                    close_window(window)
            try:
                self.process.wait(timeout=15)
                return self.finish("closed")
            except subprocess.TimeoutExpired:
                continue
        return self.kill()

    def kill(self) -> str:
        self.process.terminate()
        try:
            self.process.wait(timeout=20)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=20)
        return self.finish("killed")

    def finish(self, how: str) -> str:
        self.output.close()
        return how

    def wait(self, timeout: float) -> int | None:
        try:
            status = self.process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            return None
        self.output.close()
        return status


# ------------------------------------------------------------------------ checks


class Context:
    """What a check works with: the world, the application, its record."""

    def __init__(self, world: World, app: dict, record: dict, command: list[str], klass: str,
                 evidence: pathlib.Path) -> None:
        self.world = world
        self.app = app
        self.record = record
        self.command = command
        self.klass = klass
        self.evidence = evidence
        self.shared = world.shared / app["id"]
        self.shared.mkdir(parents=True, exist_ok=True)

    def grant(self) -> None:
        done = self.world.tur_run(["prefix", "grant", self.app["app_id"], str(self.shared), "--read-write"])
        if done.returncode != 0:
            raise RuntimeError("the shared directory could not be granted: " + tail(done.stderr, 3))

    def tool(self, name: str) -> str:
        """A profile for another program the application installed beside
        its main one, launched through tur-runtime like the main one."""
        directory = pathlib.Path(self.record["main_executable"]).parent
        found = [entry for entry in directory.iterdir() if entry.name.lower() == name.lower()]
        if not found:
            raise RuntimeError(f"{name} was not installed beside the main program")
        profile = json.loads(pathlib.Path(self.record["profile"]).read_text())
        profile["binary"] = str(found[0])
        path = self.world.profiles / f"{self.app['id']}-{name}.json"
        path.write_text(json.dumps(profile, indent=2))
        return str(path)

    def run_tool(self, profile: str, arguments: list[str], timeout: int = 300) -> subprocess.CompletedProcess:
        return self.world.tur_run(["launch", profile, "--", *arguments], timeout=timeout)


def check_seven_zip(context: Context) -> str:
    """7-Zip's command-line program compresses and extracts; the result is
    verified by another implementation and by hash."""
    context.grant()
    source = context.shared / "input"
    source.mkdir(exist_ok=True)
    data = os.urandom(200_000) + b"TUR compatibility test. " * 20_000
    (source / "data.bin").write_bytes(data)
    (source / "note.txt").write_text("7-Zip under tur-runtime\n")
    wanted = hashlib.sha256(data).hexdigest()
    tool = context.tool(context.app["check"]["tool"])

    archive = context.shared / "archive.zip"
    done = context.run_tool(tool, ["a", "-tzip", windows_path(archive), windows_path(source)])
    if done.returncode != 0 or not archive.is_file():
        raise RuntimeError(f"7z a -tzip exited with {done.returncode}: {tail(done.stdout + done.stderr, 4)}")
    with zipfile.ZipFile(archive) as opened:
        if opened.testzip() is not None:
            raise RuntimeError("Python's zipfile found a damaged member in the archive 7-Zip wrote")
        if hashlib.sha256(opened.read("input/data.bin")).hexdigest() != wanted:
            raise RuntimeError("the file read back from the zip archive differs from the original")

    packed = context.shared / "archive.7z"
    done = context.run_tool(tool, ["a", windows_path(packed), windows_path(source)])
    if done.returncode != 0 or not packed.is_file():
        raise RuntimeError(f"7z a exited with {done.returncode}: {tail(done.stdout + done.stderr, 4)}")
    out = context.shared / "out"
    done = context.run_tool(tool, ["x", windows_path(packed), "-o" + windows_path(out), "-y"])
    extracted = out / "input" / "data.bin"
    if done.returncode != 0 or not extracted.is_file():
        raise RuntimeError(f"7z x exited with {done.returncode}: {tail(done.stdout + done.stderr, 4)}")
    if sha256_of(extracted) != wanted:
        raise RuntimeError("the file extracted from the 7z archive differs from the original")
    ratio = packed.stat().st_size / len(data)
    return (f"7z.exe wrote a zip archive that Python's zipfile reads back intact, and a 7z archive "
            f"({ratio:.0%} of the input) that it extracts again; SHA-256 of the 680 kB test file round-trips")


def check_open_file(context: Context) -> str:
    """The program is given a file on its command line and shows it."""
    context.grant()
    name = context.app["check"]["file"]
    document = context.shared / name
    document.write_text("Opened by Notepad++ under tur-runtime.\r\nSecond line.\r\n")
    launch = Launch(context.world, context.command, [windows_path(document)],
                    context.evidence / "check-launch.txt")
    window, seconds = launch.wait_for_window(context.klass, context.app["check"]["title"], 90)
    if window is None:
        titles = [found.title for found in launch.new_windows()]
        launch.kill()
        raise RuntimeError(f"no window titled after the file appeared within 90 s; windows seen: {titles}")
    time.sleep(2)
    screenshot(context.evidence / "check.png")
    ended = launch.close(context.klass)
    return (f"given a file on its command line, it opened it: its window is titled {window.title!r} "
            f"after {seconds:.1f} s ({ended})")


def one_connection(text: bytes, hold: threading.Event) -> tuple[int, dict]:
    """A listener on the loopback interface that accepts one connection,
    sends `text`, and closes when `hold` is set."""
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.bind(("127.0.0.1", 0))
    server.listen(1)
    server.settimeout(120)
    seen: dict = {"accepted": False}

    def serve() -> None:
        try:
            connection, _ = server.accept()
        except OSError:
            return
        finally:
            server.close()
        seen["accepted"] = True
        with connection:
            connection.sendall(text)
            hold.wait(120)

    threading.Thread(target=serve, daemon=True).start()
    return server.getsockname()[1], seen


def check_plink(context: Context) -> str:
    """PuTTY's command-line connection tool connects to a listener on this
    machine and prints what it is sent."""
    nonce = "tur-check-" + os.urandom(6).hex()
    hold = threading.Event()
    hold.set()
    port, seen = one_connection(nonce.encode() + b"\r\n", hold)
    tool = context.tool(context.app["check"]["tool"])
    done = context.run_tool(tool, ["-raw", "-batch", "-P", str(port), "127.0.0.1"], timeout=120)
    if not seen["accepted"]:
        raise RuntimeError(f"plink made no connection (exit {done.returncode}): {tail(done.stdout + done.stderr, 4)}")
    if nonce not in done.stdout:
        raise RuntimeError(f"plink connected and did not print what it was sent (exit {done.returncode}): "
                           f"{tail(done.stdout + done.stderr, 4)}")
    return "plink.exe made a raw TCP connection to a listener on the runner and printed the line it was sent"


def check_putty(context: Context) -> str:
    """PuTTY connects to a listener on this machine and opens its terminal
    window on the connection."""
    nonce = "tur-check-" + os.urandom(6).hex()
    hold = threading.Event()
    port, seen = one_connection(f"Hello from the runner. {nonce}\r\n".encode(), hold)
    launch = Launch(context.world, context.command, ["-raw", "-P", str(port), "127.0.0.1"],
                    context.evidence / "check-launch.txt")
    window, seconds = launch.wait_for_window(context.klass, context.app["check"]["title"], 90)
    if window is None or not seen["accepted"]:
        titles = [found.title for found in launch.new_windows()]
        hold.set()
        launch.kill()
        raise RuntimeError(f"no terminal window on the connection within 90 s (connection accepted: "
                           f"{seen['accepted']}); windows seen: {titles}")
    time.sleep(2)
    screenshot(context.evidence / "check.png")
    # The far end closes: a clean end of session, on which PuTTY exits.
    hold.set()
    status = launch.wait(20)
    ended = "exited when the connection closed" if status is not None else launch.close(context.klass)
    return (f"putty.exe made a raw TCP connection to a listener on the runner and opened its terminal "
            f"window, titled {window.title!r}, after {seconds:.1f} s ({ended})")


def check_inno(context: Context) -> str:
    """Inno Setup's command-line compiler builds an installer, which
    tur-runtime then recognises as an Inno Setup installer."""
    context.grant()
    (context.shared / "hello.txt").write_text("installed by a setup that was compiled under tur-runtime\r\n")
    script = context.shared / "hello.iss"
    script.write_text("\r\n".join([
        "[Setup]", "AppName=TUR Check", "AppVersion=1.0", "DefaultDirName={autopf}\\TUR Check",
        "OutputDir=out", "OutputBaseFilename=tur-check-setup", "",
        "[Files]", 'Source: "hello.txt"; DestDir: "{app}"', ""]))
    tool = context.tool(context.app["check"]["tool"])
    done = context.run_tool(tool, [windows_path(script)], timeout=600)
    built = context.shared / "out" / "tur-check-setup.exe"
    if done.returncode != 0 or not built.is_file():
        raise RuntimeError(f"ISCC exited with {done.returncode}: {tail(done.stdout + done.stderr, 6)}")
    status, identified, _ = context.world.tur_json(["installer", "identify", str(built), "--json"])
    kind = identified.get("installer", {}).get("kind")
    if status != 0 or kind != "inno-setup":
        raise RuntimeError(f"the installer ISCC built was identified as {kind!r}")
    return (f"ISCC.exe compiled a script into a {built.stat().st_size // 1024} kB installer, which "
            f"`tur-runtime installer identify` recognises as Inno Setup")


def check_vlc(context: Context) -> str:
    """VLC decodes a sound file, resamples it and writes it out again,
    without its window; the result is measured."""
    context.grant()
    tone = context.shared / "tone.wav"
    rate, frequency, seconds = 44_100, 440, 2
    with wave.open(str(tone), "wb") as written:
        written.setnchannels(1)
        written.setsampwidth(2)
        written.setframerate(rate)
        written.writeframes(b"".join(
            struct.pack("<h", int(20_000 * math.sin(2 * math.pi * frequency * index / rate)))
            for index in range(rate * seconds)))
    out = context.shared / "out.wav"
    target = windows_path(out).replace("\\", "/")
    chain = f"#transcode{{acodec=s16l,samplerate=22050,channels=1}}:std{{access=file,mux=wav,dst={target}}}"
    launch = Launch(context.world, context.command,
                    ["-I", "dummy", "--dummy-quiet", "--no-qt-privacy-ask", "--no-video",
                     windows_path(tone), "--sout=" + chain, "vlc://quit"],
                    context.evidence / "check-launch.txt")
    status = launch.wait(180)
    if status is None:
        launch.kill()
        raise RuntimeError("VLC did not finish converting a two-second sound file within 180 s")
    if not out.is_file() or out.stat().st_size < 1000:
        raise RuntimeError(f"VLC exited with {status} and wrote no usable file: "
                           f"{tail(launch.log_path.read_text(errors='replace'), 5)}")
    with wave.open(str(out), "rb") as read:
        frames = read.readframes(read.getnframes())
        got_rate, channels, width = read.getframerate(), read.getnchannels(), read.getsampwidth()
    if (got_rate, channels, width) != (22_050, 1, 2):
        raise RuntimeError(f"the file VLC wrote is {got_rate} Hz, {channels} channel(s), {8 * width}-bit")
    samples = struct.unpack(f"<{len(frames) // 2}h", frames[:len(frames) // 2 * 2])
    length = len(samples) / got_rate
    crossings = sum(1 for before, after in zip(samples, samples[1:]) if (before < 0) != (after < 0))
    measured = crossings / 2 / length if length else 0.0
    if not (1.5 <= length <= 2.5) or abs(measured - frequency) > 0.05 * frequency:
        raise RuntimeError(f"the file VLC wrote is {length:.2f} s of a {measured:.0f} Hz tone; "
                           f"{seconds} s of {frequency} Hz went in")
    return (f"vlc.exe, without its window, decoded a {seconds} s 440 Hz WAV, resampled it from 44.1 to "
            f"22.05 kHz and wrote {length:.2f} s of a {measured:.0f} Hz tone, measured from the samples")


CHECKS = {
    "seven-zip-round-trip": check_seven_zip,
    "open-file-title": check_open_file,
    "plink-raw": check_plink,
    "putty-raw": check_putty,
    "inno-compile": check_inno,
    "vlc-transcode": check_vlc,
}


# --------------------------------------------------------------- one application


def download(world: World, app: dict, result: dict) -> pathlib.Path | None:
    directory = world.downloads / app["id"]
    directory.mkdir(parents=True, exist_ok=True)
    target = directory / app["url"].rsplit("/", 1)[1]
    if target.is_file() and target.stat().st_size == app["size"] and sha256_of(target) == app["sha256"]:
        result["download"] = {"ok": True, "seconds": 0.0, "size": app["size"], "sha256": app["sha256"],
                              "cached": True}
        return target
    started = time.monotonic()
    try:
        request = urllib.request.Request(app["url"], headers={"User-Agent": "tur-ci/1.0"})
        with urllib.request.urlopen(request, timeout=300) as response, target.open("wb") as written:
            shutil.copyfileobj(response, written)
    except OSError as error:
        result["download"] = {"ok": False, "detail": f"could not be downloaded: {error}"}
        return None
    size, digest = target.stat().st_size, sha256_of(target)
    ok = size == app["size"] and digest == app["sha256"]
    result["download"] = {"ok": ok, "seconds": round(time.monotonic() - started, 1), "size": size,
                          "sha256": digest}
    if not ok:
        result["download"]["detail"] = (f"the file is {size} bytes with SHA-256 {digest}; pinned are "
                                        f"{app['size']} bytes and {app['sha256']}. It was not run")
        target.unlink()
        return None
    return target


def stage(result: dict, name: str, status: str, **fields) -> None:
    result[name] = {"status": status, **fields}
    line = f"{status.upper():8} {result['id']}: {name}"
    detail = fields.get("detail")
    log(line + (f"\n         {detail}" if detail else ""))


def copy_evidence(source: pathlib.Path, target: pathlib.Path) -> None:
    try:
        if source.is_dir():
            shutil.copytree(source, target, dirs_exist_ok=True)
        elif source.is_file():
            shutil.copy2(source, target)
    except OSError as error:
        log(f"  could not keep {source}: {error}")


def leftovers(world: World, app_id: str, windows_before: set[str]) -> list[str]:
    """Everything of an application that is still there after its removal."""
    found = [f"{path} still exists" for path in world.runtime_paths(app_id) if path.exists()]
    for root in (world.data, world.state):
        for directory, names, files in os.walk(root):
            for name in names + files:
                if app_id in name:
                    found.append(f"{pathlib.Path(directory) / name} is named after the application")
    # Give a Wine server that is on its way out the time to go.
    for _ in range(20):
        servers = run(["pgrep", "-a", "wineserver"], timeout=10).stdout.strip()
        if not servers:
            break
        time.sleep(0.5)
    else:
        found.append(f"a Wine server is still running: {servers}")
    stray = [window.describe() for window in list_windows() if window.id not in windows_before]
    if stray:
        found.append(f"windows are still open: {stray}")
    return sorted(set(found))


# Where Wine, or an installer run without a sandbox, leaves things in a home
# directory: its default prefix, menu entries, icons, file associations,
# desktop shortcuts.
HOST_PLACES = (".wine", ".local/share/applications", ".local/share/icons", ".local/share/mime",
               ".local/share/desktop-directories", ".config/menus", ".cache/wine", "Desktop")


def host_traces() -> dict[str, list[str]]:
    """What is in those places in the home directory of the user running
    the test, which no application was granted."""
    home = pathlib.Path(os.environ["HOME"])
    traces = {}
    for place in HOST_PLACES:
        names = []
        for directory, directories, files in os.walk(home / place):
            names += [str(pathlib.Path(directory) / name) for name in directories + files]
            if len(names) > 5000:
                break
        traces[place] = sorted(names)
    return traces


def test_application(world: World, app: dict) -> dict:
    app_id = app["app_id"]
    evidence = world.evidence / "apps" / app["id"]
    evidence.mkdir(parents=True, exist_ok=True)
    result: dict = {"id": app["id"], "app_id": app_id, "name": app["name"], "version": app["version"],
                    "toolkit": app["toolkit"], "licence": app["licence"], "url": app["url"],
                    "expected_kind": app["expected_kind"], "notes": []}
    for name in STAGES:
        result[name] = {"status": "not-run"}
    windows_before = {window.id for window in list_windows()}

    log(f"\n=== {app['name']} {app['version']} ({app['id']}) ===")
    installer = download(world, app, result)
    if installer is None:
        result["notes"].append("Not tested: " + result["download"]["detail"])
        log("NOT RUN  " + result["download"]["detail"])
        return result

    # What kind of installer it is, from its content.
    status, identified, stderr = world.tur_json(["installer", "identify", str(installer), "--json"])
    kind = identified.get("installer", {}).get("kind")
    result["installer_kind"] = kind
    result["identify"] = {"ok": status == 0 and kind == app["expected_kind"], "kind": kind,
                          "evidence": identified.get("installer", {}).get("evidence"),
                          "architecture": identified.get("installer", {}).get("architecture")}
    (evidence / "identify.json").write_text(json.dumps(identified, indent=2))
    if not result["identify"]["ok"]:
        stage(result, "install", "fail", detail=f"identified as {kind!r}, expected {app['expected_kind']!r}: "
                                                 f"{tail(stderr, 2)}")
        return result

    # Install.
    started = time.monotonic()
    status, installed, stderr = world.tur_json(
        ["app", "install", str(installer), "--app-id", app_id, "--launcher", world.tur, "--timeout", "900",
         "--json", *app.get("install_options", []), *world.install_options], timeout=1500)
    seconds = round(time.monotonic() - started, 1)
    (evidence / "install.json").write_text(json.dumps(installed, indent=2))
    copy_evidence(world.state / "tur/runtime/logs" / app_id, evidence / "logs")
    record = installed.get("record")
    if status != 0 or not record:
        error = installed.get("error", {})
        stage(result, "install", "fail", seconds=seconds, code=error.get("code"),
              detail=f"exit status {status}: {error.get('message') or tail(stderr, 6)}")
        for path in world.runtime_paths(app_id):
            if path.exists():
                result["notes"].append(f"The failed installation left {path}.")
        return result
    problems = []
    main = pathlib.Path(record["main_executable"])
    if not main.is_file():
        problems.append("the recorded main program does not exist")
    wanted_main = app.get("main_executable")
    if wanted_main and main.name.lower() != wanted_main.lower():
        problems.append(f"the main program chosen is {main.name}, expected {wanted_main}")
    if record["install"]["source"]["sha256"] != app["sha256"]:
        problems.append("the recorded SHA-256 is not the installer's")
    if json.loads((world.data / "tur/runtime/prefixes" / app_id / "application.json").read_text()) != record:
        problems.append("the record on disk is not the one reported")
    # Case folding: what the record says, and what the directory says.
    drive_c = world.data / "tur/runtime/prefixes" / app_id / "prefix/drive_c"
    attributes = run(["lsattr", "-d", str(drive_c)], env=dict(os.environ, LC_ALL="C"), timeout=30)
    folded = ("F" in attributes.stdout.split()[0]) if attributes.returncode == 0 and attributes.stdout.strip() else None
    result["case_folding"] = {"record": record.get("case_folding"), "lsattr_casefolded": folded,
                              "warnings": installed.get("warnings")}
    if (record.get("case_folding") or {}).get("casefolded", False) != bool(folded):
        problems.append(f"the record says {record.get('case_folding')} and lsattr says casefolded: {folded}")
    if world.expect_casefold and bool(folded) != (world.expect_casefold == "yes"):
        problems.append(f"drive_c is {'' if folded else 'not '}case-insensitive; this pass expects "
                        f"{'it' if world.expect_casefold == 'yes' else 'the opposite'}")

    # The launcher entry and the icon.
    launcher: dict = {"desktop_entry": record["launcher"]["desktop_entry"], "icon": None}
    command, klass = None, main.name.lower()
    try:
        entry = read_desktop_entry(pathlib.Path(record["launcher"]["desktop_entry"]))
        command = exec_command(entry)
        klass = entry.get("StartupWMClass", klass)
        launcher.update(name=entry.get("Name"), exec=entry.get("Exec"), window_class=klass,
                        icon_key=entry.get("Icon"), would_install=installed.get("launcher", {}))
        if command[:2] != [world.tur, "launch"] or command[2] != record["profile"]:
            problems.append(f"the launcher entry starts {command}")
        copy_evidence(pathlib.Path(record["launcher"]["desktop_entry"]), evidence / "launcher.desktop")
        if record["launcher"].get("icon"):
            icon = pathlib.Path(record["launcher"]["icon"])
            launcher["icon"] = check_png(icon)
            launcher["icon"]["from"] = installed.get("launcher", {}).get("icon")
            # The icon is the publisher's work and stays on the machine it
            # was extracted on (windows-guest.md 6.7): what is kept is its
            # digest, not the file.
            launcher["icon"]["sha256"] = sha256_of(icon)
            if entry.get("Icon") != str(icon):
                problems.append("the launcher entry does not name the icon file")
        else:
            result["notes"].append("The main program has no icon that could be read; the entry names a generic one.")
    except (OSError, ValueError, KeyError, IndexError, struct.error, zlib.error) as error:
        problems.append(f"the launcher entry or its icon is not usable: {error}")
    result["launcher"] = launcher
    result["record"] = {key: record.get(key) for key in
                        ("name", "version", "publisher", "main_executable_windows", "main_executable_reason",
                         "runtime_version")}
    result["record"]["uninstall"] = record["uninstall"]
    result["record"]["shortcuts"] = len(record.get("shortcuts", []))
    result["record"]["executables"] = len(record.get("executables", []))
    stage(result, "install", "fail" if problems else "pass", seconds=seconds,
          breakdown=installed.get("seconds"), detail="; ".join(problems) or None)
    copy_evidence(world.data / "tur/runtime/prefixes" / app_id / "application.json", evidence / "application.json")
    copy_evidence(world.data / "tur/runtime/prefixes" / app_id / "profile.json", evidence / "profile.json")

    if command is not None:
        # Start it from its launcher entry and wait for a window of its own.
        try:
            launch = Launch(world, command, app.get("launch_arguments", []), evidence / "launch.txt")
            window, seconds = launch.wait_for_window(klass, None, 120)
            if window is None:
                seen = [found.describe() for found in launch.new_windows()]
                exited = launch.process.poll()
                launch.kill()
                stage(result, "launch", "fail", seconds=round(seconds, 1),
                      detail=f"no window of class {klass!r} within 120 s (exit status so far: {exited}); "
                             f"windows seen: {seen}; output: {tail(launch.log_path.read_text(errors='replace'), 4)}")
            else:
                # Let it finish drawing, then look at every window it has
                # open: the one with the expected title, or the largest.
                time.sleep(3)
                own = [found for found in launch.new_windows()
                       if klass.lower() in (found.instance.lower(), found.klass.lower())
                       and found.width >= 120 and found.height >= 60 and is_viewable(found)]
                wanted = app.get("window_title")
                titled = [found for found in own if wanted and re.search(wanted, found.title)]
                largest = sorted(own, key=lambda found: found.width * found.height, reverse=True)
                shown = (titled or largest or [window])[0]
                others = [found.describe() for found in own if found.id != shown.id]
                screenshot(evidence / "launch.png")
                ended = launch.close(klass)
                if wanted and not titled:
                    stage(result, "launch", "fail", seconds_to_window=round(seconds, 1), window=shown.describe(),
                          other_windows=others, closed=ended,
                          detail=f"a window of class {klass!r} appeared, and none is titled like {wanted!r}: "
                                 f"{[found.title for found in own]}")
                else:
                    stage(result, "launch", "pass", seconds_to_window=round(seconds, 1), window=shown.describe(),
                          other_windows=others, closed=ended)
                if others:
                    result["notes"].append("It also opened: " + ", ".join(repr(found["title"]) for found in others))
                if ended == "killed":
                    result["notes"].append("Its window did not close when asked; the application was stopped.")
        except Exception as error:  # noqa: BLE001 - a harness error is a result
            stage(result, "launch", "fail", detail=f"harness error: {error}")
            traceback.print_exc()

        # One thing the program is for.
        check = CHECKS[app["check"]["kind"]]
        try:
            context = Context(world, app, record, command, klass, evidence)
            started = time.monotonic()
            description = check(context)
            stage(result, "check", "pass", seconds=round(time.monotonic() - started, 1), detail=description)
        except Exception as error:  # noqa: BLE001 - a failed check is a result
            stage(result, "check", "fail", detail=str(error))
            if not isinstance(error, RuntimeError):
                traceback.print_exc()
        result["check"]["what"] = (check.__doc__ or "").strip().replace("\n    ", " ")

    # Remove it, and look for anything left.
    copy_evidence(world.state / "tur/runtime/logs" / app_id, evidence / "logs")
    started = time.monotonic()
    status, removed, stderr = world.tur_json(["app", "uninstall", app_id, "--timeout", "600", "--json"],
                                             timeout=900)
    seconds = round(time.monotonic() - started, 1)
    (evidence / "uninstall.json").write_text(json.dumps(removed, indent=2))
    left = leftovers(world, app_id, windows_before)
    uninstaller = removed.get("uninstaller", {})
    if status != 0 or left:
        error = removed.get("error", {})
        stage(result, "uninstall", "fail", seconds=seconds, uninstaller=uninstaller, left=left,
              detail=f"exit status {status}: {error.get('message') or tail(stderr, 4)}; left: {left}")
    elif uninstaller.get("ran") and not uninstaller.get("succeeded"):
        stage(result, "uninstall", "partial", seconds=seconds, uninstaller=uninstaller, left=left,
              detail="the application's own uninstaller did not succeed; the application was removed all the "
                     "same and nothing is left")
    else:
        stage(result, "uninstall", "pass", seconds=seconds, uninstaller=uninstaller, left=left)
    if uninstaller.get("ran"):
        after = uninstaller.get("left_in_prefix", {})
        kept = len(after.get("programs_left", []))
        if kept or after.get("uninstall_entry_present"):
            result["notes"].append(
                f"Its own uninstaller left {kept} of {result['record']['executables']} programs"
                + (" and its uninstall entry" if after.get("uninstall_entry_present") else "")
                + " in the prefix, which was then deleted.")
    else:
        result["notes"].append("It has no uninstaller; removing it is deleting its prefix.")

    # Whatever happened, the next application starts from nothing.
    for path in world.runtime_paths(app_id):
        if path.exists():
            shutil.rmtree(path, ignore_errors=True)
    run(["pkill", "-9", "wineserver"], timeout=10)
    return result


# ------------------------------------------------------------------ the report


def cell(result: dict, name: str) -> str:
    entry = result[name]
    status = entry["status"]
    if status == "not-run":
        return "not run"
    if name == "install" and "seconds" in entry:
        return f"{status} ({entry['seconds']} s)"
    if name == "launch" and "seconds_to_window" in entry:
        return f"{status} (window after {entry['seconds_to_window']} s)"
    if name == "uninstall" and "seconds" in entry:
        return f"{status} ({entry['seconds']} s)"
    return status


def notes_for(result: dict, app: dict) -> str:
    notes = []
    for name in STAGES:
        entry = result[name]
        if entry["status"] in ("fail", "partial") and entry.get("detail"):
            notes.append(f"{name}: {entry['detail']}")
    if result["check"]["status"] == "pass":
        notes.append(result["check"]["detail"])
    window = result["launch"].get("window")
    if window:
        notes.append(f"Window: {window['title']!r}, {window['width']}x{window['height']}.")
    notes.extend(result["notes"])
    known = app.get("known_issue")
    if known:
        notes.append(f"Known: {known}")
    text = " ".join(note.rstrip(".") + "." for note in notes if note)
    return text.replace("|", "\\|").replace("\n", " ")


def write_report(world: World, apps: list[dict], results: list[dict], environment: dict) -> list[str]:
    lines = [f"### {world.title}", "",
             f"Wine: `{environment['wine']}` from Ubuntu's archive, on a GitHub-hosted runner "
             f"({environment['os']}, kernel {environment['kernel']}). **This is not the TUR image**, which ships "
             "no Wine yet. No GPU: an X server without a screen (Xvfb), everything drawn in software. "
             "No Microsoft component, redistributable runtime or winetricks verb was installed.", "",
             "| Application | Version | Installer kind | Install | Launch | Check | Uninstall | Notes |",
             "|---|---|---|---|---|---|---|---|"]
    differences = []
    by_id = {app["id"]: app for app in apps}
    for result in results:
        app = by_id[result["id"]]
        lines.append("| " + " | ".join([
            result["name"], result["version"], result.get("installer_kind") or "not identified",
            cell(result, "install"), cell(result, "launch"), cell(result, "check"), cell(result, "uninstall"),
            notes_for(result, app)]) + " |")
        for name in STAGES:
            wanted = app.get("expected", {}).get(name)
            if wanted and result[name]["status"] != wanted:
                differences.append(f"{result['id']}: {name} is {result[name]['status']!r}, expected {wanted!r}")
    escaped = environment.get("outside_the_sandbox", [])
    if escaped:
        differences.append(f"files appeared in the home directory, outside every sandbox: {escaped[:20]}")
    lines += ["", "Outside the sandbox: " + (
        "**files appeared in the home directory of the user running the test**: " + ", ".join(escaped[:20])
        if escaped else
        "nothing appeared in the home directory of the user running the test, in any of the places Wine or "
        "an installer would use without one (" + ", ".join("~/" + place for place in HOST_PLACES) + ").")]
    lines += ["", "pass: the step worked. fail: it did not; the note says what happened. partial: the "
                  "application was removed, and its own uninstaller did not succeed. Times are wall-clock on "
                  "the runner. A pass is a measurement of these steps for this version, not a statement that "
                  "the application is compatible.", ""]
    if differences:
        lines += ["**Results that differ from what tests/windows-runtime/apps.json expects:**", ""]
        lines += [f"- {difference}" for difference in differences]
        lines.append("")
    (world.evidence / "compatibility.md").write_text("\n".join(lines))
    (world.evidence / "results.json").write_text(json.dumps(
        {"environment": environment, "results": results, "differences": differences}, indent=2))
    return differences


def catalogue_evidence(world: World, apps: list[dict], results: list[dict], environment: dict) -> None:
    """For applications that have a catalogue entry: a compatibility record
    in the form catalogue/README.md describes, for the entry's owner to take
    or leave. Nothing under catalogue/ is changed by this test."""
    directory = world.evidence / "catalogue"
    directory.mkdir(exist_ok=True)
    by_id = {app["id"]: app for app in apps}
    for result in results:
        app = by_id[result["id"]]
        if "catalogue_id" not in app:
            continue
        statuses = [result[name]["status"] for name in STAGES]
        untested = app.get("core_function_untested")
        if "not-run" in statuses:
            rating = "unknown"
        elif all(status == "pass" for status in statuses) and untested:
            # What the application is mainly for could not be tried here.
            # The steps are recorded; the rating stays unknown.
            rating = "unknown"
        elif all(status == "pass" for status in statuses):
            rating = "good"
        elif result["install"]["status"] == "pass" and result["launch"]["status"] == "pass":
            rating = "usable" if result["check"]["status"] == "pass" else "poor"
        else:
            rating = "broken"
        summary = ", ".join(f"{name} {result[name]['status']}" for name in STAGES)
        notes = (f"Automated test of {result.get('installer_kind')} installer {app['url'].rsplit('/', 1)[1]}: "
                 f"{summary}. {environment['wine']} from Ubuntu on a hosted runner, software rendering; "
                 "not the TUR image. One window and one function were looked at.")
        if untested:
            notes += f" Not tested: {untested}."
        text = "\n".join([
            "# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0",
            f"# Proposed record for catalogue/{app['catalogue_id']}.toml, under its Windows edition.",
            f"# Measured by tests/windows-runtime/apps.py, application {result['id']}.",
            "[[editions.records]]",
            'route = "wine"',
            f'rating = "{rating}"',
            'reported_by = "tur-test"',
            'test_report = "docs/reports/windows-runtime-2.md"',
            f'version_tested = "{result["version"]}"',
            f'date = "{time.strftime("%Y-%m-%d", time.gmtime())}"',
            f"notes = {json.dumps(notes)}",
            ""])
        (directory / f"{app['catalogue_id']}.{result['id']}.record.toml").write_text(text)


def describe_environment() -> dict:
    def first_line(command: list[str]) -> str:
        return (run(command, timeout=60).stdout.strip().splitlines() or ["not available"])[0]

    release = {}
    for line in pathlib.Path("/etc/os-release").read_text().splitlines():
        key, _, value = line.partition("=")
        release[key] = value.strip('"')
    packages = run(["dpkg-query", "-W", "-f", "${Package}:${Architecture} ${Version}\n", "wine", "wine64",
                    "wine32", "libwine", "bubblewrap", "xvfb", "openbox", "unzip"], timeout=60).stdout
    return {"wine": first_line(["wine", "--version"]), "os": release.get("PRETTY_NAME", "unknown"),
            "kernel": first_line(["uname", "-r"]), "bubblewrap": first_line(["bwrap", "--version"]),
            "runner_image": f"{os.environ.get('ImageOS', '?')} {os.environ.get('ImageVersion', '?')}",
            "packages": sorted(set(packages.split("\n")) - {""}),
            "display": "Xvfb 1280x800x24 with openbox; no GPU, software rendering",
            "tur_image": False}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--tur-runtime", required=True)
    parser.add_argument("--apps", required=True, type=pathlib.Path)
    parser.add_argument("--work-dir", required=True, type=pathlib.Path)
    parser.add_argument("--evidence", required=True, type=pathlib.Path)
    parser.add_argument("--only", default="", help="comma-separated application ids")
    parser.add_argument("--install-option", action="append", default=[],
                        help="an option added to every app install, such as --casefold")
    parser.add_argument("--expect-casefold", choices=("yes", "no"),
                        help="whether every prefix's drive_c must be case-insensitive")
    parser.add_argument("--downloads", type=pathlib.Path, help="a directory of downloads shared between passes")
    parser.add_argument("--title", help="the heading of the table")
    arguments = parser.parse_args()

    apps = json.loads(arguments.apps.read_text())["applications"]
    if arguments.only:
        apps = [app for app in apps if app["id"] in arguments.only.split(",")]
    world = World(arguments.tur_runtime, arguments.work_dir.resolve(), arguments.evidence.resolve(),
                  downloads=arguments.downloads.resolve() if arguments.downloads else None,
                  install_options=arguments.install_option, expect_casefold=arguments.expect_casefold,
                  title=arguments.title)
    environment = describe_environment()
    traces_before = host_traces()
    log(json.dumps(environment, indent=2))
    processes = start_display(world.evidence)
    results = []
    try:
        for app in apps:
            try:
                results.append(test_application(world, app))
            except Exception as error:  # noqa: BLE001 - one application must not stop the others
                traceback.print_exc()
                broken = {"id": app["id"], "name": app["name"], "version": app["version"],
                          "notes": [f"The test harness failed: {error}"]}
                broken.update({name: {"status": "not-run"} for name in STAGES})
                results.append(broken)
    finally:
        for process in processes:
            process.terminate()
    # Nothing may have appeared in the home directory: every application ran
    # in a sandbox that does not contain it.
    traces_after = host_traces()
    environment["outside_the_sandbox"] = sorted(
        name for place in HOST_PLACES for name in set(traces_after[place]) - set(traces_before[place]))
    differences = write_report(world, apps, results, environment)
    catalogue_evidence(world, apps, results, environment)
    log("\n" + (world.evidence / "compatibility.md").read_text())
    return 1 if differences else 0


if __name__ == "__main__":
    sys.exit(main())
