#!/usr/bin/env python3
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
"""End-to-end test of uos-runtime with real binaries, a real Wine and a real sandbox.

Run by .github/workflows/windows-runtime.yml on a hosted Ubuntu runner, after
build.sh has compiled the test programs. It is never run on a development
machine (AGENTS.md).

    e2e.py --uos-runtime BINARY --build-dir DIR --work-dir DIR --results FILE

What it proves, when it passes:

  * `uos-runtime inspect` agrees with binutils about real ELF and PE files
    and with Python's zip writer about real archives (issue #12, item 1);
  * a Windows program really ran under Wine through `uos-runtime launch`,
    with its output, exit status and arguments intact;
  * the sandbox held: the program wrote where it was granted, and could not
    read files outside its grants that the same program does read when it
    is started by Wine directly (the control run);
  * a prefix is reused, its record updated, and a runtime version mismatch
    is refused.

What it does not prove: anything about real applications, about Proton (plan
tests only), or about the TUR image, whose Wine is not the one used here.

Every check is printed as PASS or FAIL as it happens. The exit status is 1 if
any check failed.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time
import zipfile

TAG = "uos-windows-test: "
APP = "local.uos-test"
GUI_APP = "local.uos-gui-test"
HOST_CANARY = "leaked-from-the-host-environment"


class Results:
    def __init__(self) -> None:
        self.checks: list[dict] = []
        self.facts: dict[str, str] = {}

    def check(self, name: str, ok: bool, detail: str = "") -> bool:
        self.checks.append({"name": name, "ok": bool(ok), "detail": detail})
        line = f"{'PASS' if ok else 'FAIL'}  {name}"
        if detail and not ok:
            line += f"\n      {detail}"
        print(line, flush=True)
        return bool(ok)

    def equal(self, name: str, actual, expected) -> bool:
        return self.check(name, actual == expected, f"expected {expected!r}, got {actual!r}")

    def fact(self, name: str, value: str) -> None:
        self.facts[name] = value
        print(f"FACT  {name}: {value}", flush=True)

    @property
    def failed(self) -> list[dict]:
        return [check for check in self.checks if not check["ok"]]


R = Results()


def section(title: str) -> None:
    print(f"\n=== {title} ===", flush=True)


def run(command: list[str], env: dict[str, str] | None = None, timeout: int = 600,
        cwd: str | None = None) -> subprocess.CompletedProcess:
    """Runs a command, captures its output as text, never raises on failure."""
    try:
        return subprocess.run(command, env=env, cwd=cwd, capture_output=True, text=True,
                              errors="replace", timeout=timeout)
    except subprocess.TimeoutExpired as error:
        out = error.stdout.decode(errors="replace") if isinstance(error.stdout, bytes) else (error.stdout or "")
        err = error.stderr.decode(errors="replace") if isinstance(error.stderr, bytes) else (error.stderr or "")
        return subprocess.CompletedProcess(command, 124, out, err + f"\n[timed out after {timeout}s]")


def tail(text: str, lines: int = 25) -> str:
    return "\n      ".join(text.strip().splitlines()[-lines:])


def tagged(output: str) -> list[str]:
    """The lines the test program printed, without the tag and without CR."""
    return [line.rstrip("\r")[len(TAG):] for line in output.splitlines() if line.startswith(TAG)]


# --------------------------------------------------------------------------- inspection


def inspect(uos: str, path: pathlib.Path) -> tuple[int, dict]:
    done = run([uos, "inspect", str(path), "--json", "--host-arch", "x86_64", "--available", "wine"])
    try:
        return done.returncode, json.loads(done.stdout)
    except json.JSONDecodeError:
        return done.returncode, {"error": {"message": done.stdout + done.stderr}}


def readelf(path: pathlib.Path) -> dict:
    header = run(["readelf", "-h", str(path)]).stdout
    segments = run(["readelf", "-l", str(path)]).stdout

    def field(name: str) -> str:
        match = re.search(rf"^\s*{re.escape(name)}:\s*(.*)$", header, re.M)
        return match.group(1).strip() if match else ""

    kind = field("Type")
    interpreter = re.search(r"Requesting program interpreter: ([^\]]+)\]", segments)
    return {
        "class": {"ELF64": "elf64", "ELF32": "elf32"}.get(field("Class"), "?"),
        "endianness": "little" if "little endian" in field("Data") else "big",
        "machine": {"Advanced Micro Devices X86-64": "x86_64", "Intel 80386": "i386"}.get(field("Machine"), "?"),
        "file_type": (
            "relocatable" if kind.startswith("REL")
            else "executable" if kind.startswith("EXEC")
            else "pie-executable" if kind.startswith("DYN") and "Position-Independent Executable" in kind
            else "shared-object" if kind.startswith("DYN")
            else "?"
        ),
        "os_abi": {"UNIX - System V": "sysv", "UNIX - GNU": "gnu-linux"}.get(field("OS/ABI"), "?"),
        "interpreter": interpreter.group(1) if interpreter else None,
        "has_dynamic": bool(re.search(r"^\s*DYNAMIC\s", segments, re.M)),
        "raw": kind,
    }


def objdump_pe(path: pathlib.Path) -> dict:
    text = run(["x86_64-w64-mingw32-objdump", "-p", str(path)]).stdout

    def hexfield(pattern: str) -> int:
        match = re.search(pattern, text, re.M)
        return int(match.group(1), 16) if match else -1

    fmt = re.search(r"file format (\S+)", text)
    return {
        "format": fmt.group(1) if fmt else "?",
        "magic": hexfield(r"^Magic\s+([0-9a-fA-F]+)"),
        "subsystem": hexfield(r"^Subsystem\s+([0-9a-fA-F]+)"),
        "characteristics": hexfield(r"^Characteristics 0x([0-9a-fA-F]+)"),
    }


def check_inspection(uos: str, build: pathlib.Path, work: pathlib.Path) -> None:
    section("Inspection of real binaries (issue #12, item 1)")

    # ELF, against readelf. (name, expected exit status, expected runtime)
    elf_cases = [
        ("native-pie", 0, "native-linux"),
        ("native-nopie", 0, "native-linux"),
        ("native-static", 0, "native-linux"),
        ("native-static-pie", 0, "native-linux"),
        ("libnative.so", 3, "unsupported"),
        ("native.o", 3, "unsupported"),
    ]
    for name, status, runtime in elf_cases:
        path = build / name
        truth = readelf(path)
        code, report = inspect(uos, path)
        detected = report.get("inspection", {}).get("detected", {})
        label = f"inspect {name}"
        R.equal(f"{label}: identified as ELF", detected.get("format"), "elf")
        for key in ("class", "endianness", "machine", "file_type", "os_abi", "interpreter"):
            R.equal(f"{label}: {key} agrees with readelf ({truth['raw']})", detected.get(key), truth[key])
        expected_linkage = (
            "unknown" if truth["file_type"] == "relocatable"
            else "dynamic" if truth["interpreter"] or (truth["has_dynamic"] and truth["file_type"] != "pie-executable")
            else "static"
        )
        R.equal(f"{label}: linkage", detected.get("linkage"), expected_linkage)
        R.equal(f"{label}: recommendation", report.get("recommendation", {}).get("runtime"), runtime)
        R.equal(f"{label}: exit status", code, status)
    # The builds are what their names say; otherwise the comparison above
    # would agree about the wrong thing.
    R.equal("gcc built a PIE by default", readelf(build / "native-pie")["file_type"], "pie-executable")
    R.equal("gcc -no-pie built a fixed-address executable", readelf(build / "native-nopie")["file_type"], "executable")
    R.equal("gcc -static built a program with no interpreter", readelf(build / "native-static")["interpreter"], None)
    R.equal("gcc -static-pie built a PIE with no interpreter",
            (readelf(build / "native-static-pie")["file_type"], readelf(build / "native-static-pie")["interpreter"]),
            ("pie-executable", None))
    R.equal("gcc -shared built a shared object", readelf(build / "libnative.so")["file_type"], "shared-object")

    # PE, against objdump. (name, machine, expected exit status, runtime, first reason)
    pe_cases = [
        ("console64.exe", "x86_64", 0, "wine", "pe-windows-application"),
        ("gui64.exe", "x86_64", 0, "wine", "pe-windows-application"),
        ("console32.exe", "i386", 0, "wine", "pe-windows-application"),
        ("library64.dll", "x86_64", 3, "unsupported", "pe-library"),
    ]
    subsystems = {2: "windows-gui", 3: "windows-console"}
    for name, machine, status, runtime, reason in pe_cases:
        path = build / name
        truth = objdump_pe(path)
        code, report = inspect(uos, path)
        detected = report.get("inspection", {}).get("detected", {})
        label = f"inspect {name}"
        R.equal(f"{label}: identified as PE", detected.get("format"), "pe")
        R.equal(f"{label}: machine", detected.get("machine"), machine)
        R.equal(f"{label}: machine agrees with objdump ({truth['format']})",
                detected.get("machine_raw"), {"pei-x86-64": 0x8664, "pei-i386": 0x014C}.get(truth["format"]))
        R.equal(f"{label}: image format agrees with objdump (magic {truth['magic']:#x})",
                detected.get("image_format"), {0x20B: "pe32-plus", 0x10B: "pe32"}.get(truth["magic"]))
        R.equal(f"{label}: subsystem number agrees with objdump", detected.get("subsystem_raw"), truth["subsystem"])
        R.equal(f"{label}: subsystem", detected.get("subsystem"), subsystems.get(truth["subsystem"]))
        R.equal(f"{label}: DLL flag agrees with objdump", detected.get("is_dll"), bool(truth["characteristics"] & 0x2000))
        R.equal(f"{label}: not .NET", detected.get("is_dotnet"), False)
        R.equal(f"{label}: needs no Windows kernel", detected.get("requires_windows_kernel"), False)
        R.equal(f"{label}: recommendation", report.get("recommendation", {}).get("runtime"), runtime)
        R.check(f"{label}: reason {reason}", reason in report.get("recommendation", {}).get("reasons", []),
                json.dumps(report.get("recommendation", {})))
        R.equal(f"{label}: exit status", code, status)
    R.equal("mingw built a console program", objdump_pe(build / "console64.exe")["subsystem"], 3)
    R.equal("mingw -mwindows built a GUI program", objdump_pe(build / "gui64.exe")["subsystem"], 2)
    R.equal("mingw -shared built a DLL", bool(objdump_pe(build / "library64.dll")["characteristics"] & 0x2000), True)

    # Zip archives written by Python's zipfile, not by hand.
    archives = work / "archives"
    archives.mkdir(parents=True, exist_ok=True)
    apk = archives / "shaped-like.apk"
    with zipfile.ZipFile(apk, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("AndroidManifest.xml", b"\x03\x00\x08\x00")
        archive.writestr("classes.dex", b"dex\n035\x00")
        archive.writestr("lib/arm64-v8a/libnative.so", (build / "libnative.so").read_bytes())
        archive.writestr("lib/x86_64/libnative.so", (build / "libnative.so").read_bytes())
        archive.writestr("res/layout/main.xml", b"<x/>")
    code, report = inspect(uos, apk)
    detected = report.get("inspection", {}).get("detected", {})
    R.equal("inspect APK-shaped zip: format", detected.get("format"), "android")
    R.equal("inspect APK-shaped zip: kind", detected.get("kind"), "apk")
    R.equal("inspect APK-shaped zip: has dex", detected.get("has_dex"), True)
    R.equal("inspect APK-shaped zip: ABIs", detected.get("native_abis"), ["arm64-v8a", "x86_64"])
    R.equal("inspect APK-shaped zip: entry count", detected.get("entries"), 5)

    plain = archives / "plain.zip"
    with zipfile.ZipFile(plain, "w", zipfile.ZIP_STORED) as archive:
        archive.writestr("readme.txt", "hello")
        archive.writestr("src/main.c", "int main(void){return 0;}")
        archive.comment = b"an archive comment, which moves the end record"
    code, report = inspect(uos, plain)
    detected = report.get("inspection", {}).get("detected", {})
    R.equal("inspect plain zip: format", detected.get("format"), "zip")
    R.equal("inspect plain zip: entry count", detected.get("entries"), 2)
    R.equal("inspect plain zip: exit status", code, 3)

    # More than 65535 members forces the zip64 end records.
    many = archives / "many.zip"
    with zipfile.ZipFile(many, "w", zipfile.ZIP_STORED) as archive:
        for index in range(70000):
            archive.writestr(f"f{index}", b"")
    code, report = inspect(uos, many)
    detected = report.get("inspection", {}).get("detected", {})
    R.equal("inspect zip64 archive with 70000 members: format", detected.get("format"), "zip")
    R.equal("inspect zip64 archive with 70000 members: entry count", detected.get("entries"), 70000)


# --------------------------------------------------------------------------- launching


class Bench:
    """The scratch world the launches run in."""

    def __init__(self, uos: str, build: pathlib.Path, work: pathlib.Path) -> None:
        self.uos = uos
        self.build = build
        self.work = work
        self.data = work / "xdg-data"
        self.state = work / "xdg-state"
        self.grant = work / "granted-rw"
        self.readonly = work / "granted-ro"
        self.outside = work / "outside"
        self.home = pathlib.Path(os.environ["HOME"])
        for directory in (self.data, self.state, self.grant, self.readonly, self.outside):
            directory.mkdir(parents=True, exist_ok=True)
        (self.readonly / "readable.txt").write_text("readable through a read-only grant\n")
        (self.outside / "secret.txt").write_text("outside-secret\n")
        # A stand-in for the user's keys, in the real home of the runner user.
        ssh = self.home / ".ssh"
        ssh.mkdir(mode=0o700, exist_ok=True)
        self.key = ssh / "uos-test-canary"
        self.key.write_text("home-key-canary\n")
        # Another application's prefix.
        other = self.data / "uos/runtime/prefixes/local.other-app/prefix"
        other.mkdir(parents=True, exist_ok=True)
        self.other_file = other / "other-apps-file.txt"
        self.other_file.write_text("another-applications-data\n")

    def app_dir(self, app: str = APP) -> pathlib.Path:
        return self.data / "uos/runtime/prefixes" / app

    def metadata_path(self, app: str = APP) -> pathlib.Path:
        return self.app_dir(app) / "metadata.json"

    def metadata(self, app: str = APP) -> dict:
        return json.loads(self.metadata_path(app).read_text())

    def outside_files(self) -> list[pathlib.Path]:
        return [self.outside / "secret.txt", self.key, self.other_file, self.metadata_path()]

    def env(self, **extra: str) -> dict[str, str]:
        """The environment uos-runtime is started in: the runner's own, plus
        scratch XDG directories and a canary that must not reach the program."""
        env = dict(os.environ)
        env.pop("DISPLAY", None)
        env.pop("WAYLAND_DISPLAY", None)
        env["XDG_DATA_HOME"] = str(self.data)
        env["XDG_STATE_HOME"] = str(self.state)
        env["UOS_TEST_HOST_CANARY"] = HOST_CANARY
        env.update(extra)
        return env

    def uos_run(self, arguments: list[str], timeout: int = 900, **extra_env: str) -> subprocess.CompletedProcess:
        done = run([self.uos, *arguments], env=self.env(**extra_env), timeout=timeout)
        print(f"$ uos-runtime {' '.join(arguments)}\n  -> exit status {done.returncode}", flush=True)
        if done.returncode != 0 and done.stderr.strip():
            print("  stderr: " + tail(done.stderr, 12), flush=True)
        return done

    def profile(self, name: str, app: str, exe: str, **members) -> str:
        profile = {
            "schema_version": 1,
            "app_id": app,
            "display_name": "UOS runtime test program",
            "binary": str(self.build / exe),
            "format": "pe",
            "architecture": "x86_64",
            "runtime": "wine",
            "prefix_id": app.split(".", 1)[1],
            "sync_backend": "auto",
            "isolation_profile": "desktop-standard",
            "fallback": "none",
            "test_status": "untested",
        }
        profile.update(members)
        path = self.work / name
        path.write_text(json.dumps(profile, indent=2))
        return str(path)

    def launch_arguments(self, profile: str, program_arguments: list[str], *options: str,
                         also_outside: tuple[pathlib.Path, ...] = ()) -> list[str]:
        outside = ";".join(str(path) for path in (*self.outside_files(), *also_outside))
        return [
            "launch", profile, *options,
            "--env", "UOS_TEST_VALUE=passed by the launch",
            "--env", f"UOS_TEST_GRANT_DIR={self.grant}",
            "--env", f"UOS_TEST_READONLY_DIR={self.readonly}",
            "--env", f"UOS_TEST_OUTSIDE_FILES={outside}",
            "--", *program_arguments,
        ]


def wine_version() -> tuple[str, str]:
    done = run(["wine", "--version"])
    build = done.stdout.strip().splitlines()[0] if done.stdout.strip() else ""
    return (build.split()[0] if build else "", build)


def control_run(bench: Bench) -> None:
    """The same program, started by Wine directly with no sandbox. It must be
    able to read the files the sandboxed run must not, or the sandbox checks
    below would prove nothing."""
    section("Control: the test program under Wine without uos-runtime")
    env = dict(os.environ)
    env.pop("DISPLAY", None)
    env["WINEPREFIX"] = str(bench.work / "control-prefix")
    # The same Wine settings uos-runtime uses when it creates a prefix, so
    # that what Wine prints here can be compared with what it prints inside
    # the sandbox.
    env["WINEDEBUG"] = "fixme-all"
    env["WINEDLLOVERRIDES"] = "mscoree,mshtml=;winemenubuilder.exe="
    env["UOS_TEST_HOST_CANARY"] = HOST_CANARY
    # The file inside the uos-runtime prefix does not exist yet; use the
    # others.
    outside = [path for path in bench.outside_files() if path.exists()]
    env["UOS_TEST_OUTSIDE_FILES"] = ";".join(str(path) for path in outside)
    started = time.time()
    done = run(["wine", str(bench.build / "console64.exe"), "0"], env=env, timeout=600)
    run(["wineserver", "--wait"], env=env, timeout=120)
    R.fact("control run time", f"{time.time() - started:.0f} s (includes creating a prefix)")
    kept = bench.work / "kept"
    kept.mkdir(parents=True, exist_ok=True)
    (kept / "control-run-stderr.txt").write_text(done.stderr)
    R.fact("Wine error lines while creating a prefix without the sandbox",
           str(sum(1 for line in done.stderr.splitlines() if ":err:" in line)))
    lines = tagged(done.stdout)
    R.check("control: the program runs under plain Wine", "hello from a Windows process" in lines,
            tail(done.stdout + done.stderr))
    for path in outside:
        R.check(f"control: without the sandbox the program reads {path}",
                any(line.startswith(f"outside-read: LEAKED {path}:") for line in lines), tail(done.stdout))
    R.check("control: without uos-runtime the caller's environment reaches the program",
            f"env UOS_TEST_HOST_CANARY={HOST_CANARY}" in lines, tail(done.stdout))


def check_prefix_and_launch(bench: Bench) -> None:
    version, build = wine_version()
    R.fact("wine --version", build)

    section("Create a prefix")
    done = bench.uos_run([
        "prefix", "create", APP, "--prefix-id", "uos-test",
        "--grant-rw", str(bench.grant), "--grant", str(bench.readonly), "--json",
    ])
    R.equal("prefix create: exit status", done.returncode, 0)
    if done.returncode != 0:
        print(tail(done.stdout + done.stderr, 60))
        operations = bench.state / "uos/runtime/logs" / APP / "operations.log"
        if operations.exists():
            print("--- operations.log ---\n" + tail(operations.read_text(errors="replace"), 60))
    operations = bench.state / "uos/runtime/logs" / APP / "operations.log"
    if operations.exists():
        R.fact("Wine error lines while creating a prefix inside the sandbox",
               str(sum(1 for line in operations.read_text(errors="replace").splitlines() if ":err:" in line)))
    prefix = bench.app_dir() / "prefix"
    R.check("prefix create: the registry exists", (prefix / "system.reg").is_file())
    R.check("prefix create: drive_c exists", (prefix / "drive_c/windows").is_dir())
    R.equal("prefix create: Wine's own prefix updating is switched off",
            (prefix / ".update-timestamp").read_text() if (prefix / ".update-timestamp").exists() else None,
            "disable\n")
    created = bench.metadata() if bench.metadata_path().exists() else {}
    R.equal("prefix create: metadata names the application", created.get("app_id"), APP)
    R.equal("prefix create: metadata records the runtime", created.get("runtime"), "wine")
    R.equal("prefix create: metadata records the exact Wine version", created.get("runtime_version"), version)
    R.equal("prefix create: metadata records the Wine build", created.get("runtime_build"), build)
    R.equal("prefix create: metadata records the architecture", created.get("architecture"), "win64")
    R.equal("prefix create: metadata records the grants", created.get("grants"), [
        {"path": str(bench.readonly), "access": "read-only"},
        {"path": str(bench.grant), "access": "read-write"},
    ])
    R.equal("prefix create: not launched yet", (created.get("launch_count"), created.get("last_run_at")), (0, None))
    R.check("prefix create: the metadata is outside the prefix",
            not str(bench.metadata_path()).startswith(str(prefix) + "/"))

    done = bench.uos_run(["prefix", "verify", APP])
    R.equal("prefix verify: the created prefix passes", done.returncode, 0)
    if done.returncode != 0:
        print(tail(done.stdout + done.stderr))

    done = bench.uos_run(["prefix", "create", APP])
    R.equal("prefix create again: refused", done.returncode, 6)
    R.check("prefix create again: code", "UOS-WINDOWS-PREFIX-002" in done.stderr, done.stderr)

    section("Launch the console program")
    profile = bench.profile("profile.json", APP, "console64.exe")
    dry = bench.uos_run(bench.launch_arguments(profile, ["42"], "--dry-run", "--json"))
    R.equal("launch --dry-run: exit status", dry.returncode, 0)
    plan = json.loads(dry.stdout).get("launch", {}) if dry.returncode == 0 else {}
    R.equal("launch --dry-run: the prefix is reused, not created", plan.get("creates_prefix"), False)
    R.equal("launch --dry-run: nothing ran", bench.metadata().get("launch_count"), 0)
    R.fact("ntsync device", str(plan.get("sync", {}).get("ntsync_device")))
    R.fact("synchronisation selected", f"{plan.get('sync', {}).get('selected')}: {plan.get('sync', {}).get('reason')}")

    first = bench.uos_run(bench.launch_arguments(profile, ["42", "two words", "C:\\path with\\back slashes"]))
    lines = tagged(first.stdout)
    print("  " + "\n  ".join(lines))
    if "hello from a Windows process" not in lines:
        print(tail(first.stdout + first.stderr, 60))
    R.check("launch: a Windows program ran under Wine through uos-runtime",
            "hello from a Windows process" in lines, tail(first.stdout + first.stderr))
    R.check("launch: it is a 64-bit Windows process", "pointer-bits=64" in lines)
    R.equal("launch: the exit status is the program's", first.returncode, 42)
    R.check("launch: the program reports the status it was asked for", "exiting with 42" in lines)
    R.check("launch: arguments arrive intact",
            {"arg[1]=42", "arg[2]=two words", "arg[3]=C:\\path with\\back slashes"} <= set(lines), str(lines))
    R.check("launch: a variable given with --env arrives", "env UOS_TEST_VALUE=passed by the launch" in lines)
    R.check("launch: the caller's environment does not reach the program",
            "env UOS_TEST_HOST_CANARY=(unset)" in lines, str(lines))

    written = bench.grant / "written-by-windows.txt"
    R.check("sandbox: the program wrote into the read-write grant", "grant-write: ok" in lines, str(lines))
    R.equal("sandbox: the written file is on the host",
            written.read_text() if written.exists() else None, "written by a Windows process\n")
    R.check("sandbox: the program read from the read-only grant",
            "readonly-read: ok readable through a read-only grant" in lines, str(lines))
    R.check("sandbox: the program could not write into the read-only grant",
            any(line.startswith("readonly-write: denied") for line in lines), str(lines))
    R.check("sandbox: nothing appeared in the read-only grant",
            sorted(path.name for path in bench.readonly.iterdir()) == ["readable.txt"])
    for path in bench.outside_files():
        R.check(f"sandbox: the program could not read {path}", f"outside-read: denied {path}" in lines, str(lines))
    R.check("sandbox: nothing was leaked", not any("LEAKED" in line for line in lines), str(lines))
    R.check("launch: first run in this prefix", "prefix-runs=1" in lines, str(lines))

    after_first = bench.metadata()
    R.equal("metadata: launch counted", after_first.get("launch_count"), 1)
    R.check("metadata: last-run time recorded", bool(after_first.get("last_run_at")))
    R.equal("metadata: creation time unchanged", after_first.get("created_at"), created.get("created_at"))
    R.equal("metadata: runtime version unchanged", after_first.get("runtime_version"), version)
    sync = after_first.get("sync", {})
    R.check("metadata: the ntsync probe result is recorded",
            sync.get("ntsync_device") in ("present", "absent", "inaccessible"), str(sync))
    R.check("metadata: the synchronisation choice and its reason are recorded",
            sync.get("selected") in ("ntsync", "server") and bool(sync.get("reason")), str(sync))
    major = re.match(r"wine-(\d+)", version)
    if major and int(major.group(1)) < 11:
        R.equal("metadata: a Wine older than 11.0 falls back to the Wine server", sync.get("selected"), "server")

    logs = sorted((bench.state / "uos/runtime/logs" / APP).glob("*Z-*.log"))
    R.equal("log: one log file for the launch", len(logs), 1)
    if logs:
        log = logs[-1].read_text(errors="replace")
        R.check("log: the program's output is captured", TAG + "hello from a Windows process" in log)
        R.check("log: the exit status is recorded", "# exit status: 42" in log, tail(log, 5))
        R.check("log: variable values are not written to the log header",
                "passed by the launch" not in log.split("\n# environment:")[0] and HOST_CANARY not in log)

    section("Launch again: the prefix is reused")
    second = bench.uos_run(bench.launch_arguments(profile, ["0"]))
    lines = tagged(second.stdout)
    R.equal("second launch: exit status", second.returncode, 0)
    R.check("second launch: the same prefix, run a second time", "prefix-runs=2" in lines, str(lines))
    after_second = bench.metadata()
    R.equal("second launch: launch counted", after_second.get("launch_count"), 2)
    R.equal("second launch: creation time unchanged", after_second.get("created_at"), created.get("created_at"))
    R.check("second launch: last-run time moved on",
            (after_second.get("last_run_at") or "") >= (after_first.get("last_run_at") or "~"),
            f"{after_first.get('last_run_at')} -> {after_second.get('last_run_at')}")
    R.check("second launch: Wine's own updating is still switched off",
            (prefix / ".update-timestamp").read_text() == "disable\n")
    R.equal("second launch: two log files", len(list((bench.state / "uos/runtime/logs" / APP).glob("*Z-*.log"))), 2)

    section("Runtime version mismatches are refused")
    pinned = bench.profile("profile-pinned.json", APP, "console64.exe", runtime_version="wine-0.0.1")
    done = bench.uos_run(bench.launch_arguments(pinned, ["0"]))
    R.equal("pinned profile, other Wine installed: refused", done.returncode, 6)
    R.check("pinned profile: code UOS-WINDOWS-RUNTIME-005", "error[UOS-WINDOWS-RUNTIME-005]" in done.stderr, done.stderr)
    R.check("pinned profile: the program did not run", not tagged(done.stdout))
    exact = bench.profile("profile-exact.json", APP, "console64.exe", runtime_version=version)
    done = bench.uos_run(bench.launch_arguments(exact, ["0"]))
    R.equal("profile pinned to the installed Wine: launched", done.returncode, 0)
    R.check("profile pinned to the installed Wine: third run", "prefix-runs=3" in tagged(done.stdout))

    done = bench.uos_run(bench.launch_arguments(profile, ["0"], "--runtime-version", "wine-99.0"))
    R.equal("prefix on another runtime version: refused", done.returncode, 6)
    R.check("prefix on another runtime version: code UOS-WINDOWS-PREFIX-006",
            "error[UOS-WINDOWS-PREFIX-006]" in done.stderr, done.stderr)
    R.check("prefix on another runtime version: the program did not run", not tagged(done.stdout))
    R.equal("refusals changed nothing", bench.metadata().get("launch_count"), 3)

    section("Migration mechanics (the Wine version is faked; the steps are real)")
    done = bench.uos_run(bench.launch_arguments(profile, ["0"], "--runtime-version", "wine-99.0", "--migrate"))
    R.equal("launch --migrate: exit status", done.returncode, 0)
    if done.returncode != 0:
        print(tail(done.stdout + done.stderr, 40))
    R.check("launch --migrate: the program ran in the same prefix", "prefix-runs=4" in tagged(done.stdout))
    migrated = bench.metadata()
    R.equal("launch --migrate: the record names the new version", migrated.get("runtime_version"), "wine-99.0")
    history = migrated.get("migrations", [])
    R.equal("launch --migrate: the move is recorded", [(m.get("from_version"), m.get("to_version")) for m in history],
            [(version, "wine-99.0")])
    backups = bench.data / "uos/runtime/backups" / APP
    archive = backups / f"{history[0]['backup']}.tar.gz" if history else backups / "missing"
    R.check("launch --migrate: a backup was taken first", archive.is_file(), str(archive))
    if archive.is_file():
        listing = run(["tar", "-tzf", str(archive)]).stdout
        R.check("backup: holds the registry, the private home and the metadata",
                "prefix/system.reg" in listing and "metadata.json" in listing and "home/" in listing, tail(listing, 5))
    done = bench.uos_run(["prefix", "migrate", APP])
    R.equal("prefix migrate back to the installed Wine: exit status", done.returncode, 0)
    R.equal("prefix migrate: the record names the installed version", bench.metadata().get("runtime_version"), version)
    R.equal("prefix migrate: both moves are recorded", len(bench.metadata().get("migrations", [])), 2)

    section("Pinned prefix")
    R.equal("prefix pin", bench.uos_run(["prefix", "pin", APP]).returncode, 0)
    done = bench.uos_run(bench.launch_arguments(profile, ["0"], "--runtime-version", "wine-99.0", "--migrate"))
    R.equal("pinned prefix: migration refused", done.returncode, 6)
    R.check("pinned prefix: code UOS-WINDOWS-PREFIX-007", "error[UOS-WINDOWS-PREFIX-007]" in done.stderr, done.stderr)
    R.equal("prefix unpin", bench.uos_run(["prefix", "unpin", APP]).returncode, 0)

    section("Backup and restore")
    done = bench.uos_run(["prefix", "backup", APP, "--json"])
    R.equal("prefix backup: exit status", done.returncode, 0)
    name = json.loads(done.stdout).get("backup", "") if done.returncode == 0 else ""
    marker = prefix / "drive_c/uos-test-runs.txt"
    R.check("the run marker is in the prefix", marker.is_file())
    marker.unlink(missing_ok=True)
    (prefix / "user.reg").unlink(missing_ok=True)
    done = bench.uos_run(["prefix", "verify", APP])
    R.equal("prefix verify: a damaged prefix is reported", done.returncode, 8)
    R.check("prefix verify: code UOS-WINDOWS-PREFIX-004", "error[UOS-WINDOWS-PREFIX-004]" in done.stderr, done.stderr)
    done = bench.uos_run(["prefix", "restore", APP, name])
    R.equal("prefix restore: exit status", done.returncode, 0)
    R.check("prefix restore: the prefix is as it was backed up", marker.is_file() and (prefix / "user.reg").is_file())
    done = bench.uos_run(bench.launch_arguments(profile, ["0"]))
    R.check("launch after restore: the run count continues", "prefix-runs=5" in tagged(done.stdout),
            str(tagged(done.stdout)))

    section("Repair")
    # A system file Wine can start without. (Without kernel32.dll in the
    # prefix Wine cannot start at all, so it cannot repair that either: that
    # case needs a restore or a reset.)
    victim = prefix / "drive_c/windows/system32/d3d11.dll"
    R.check("the system file to delete is there", victim.is_file())
    victim.unlink(missing_ok=True)
    done = bench.uos_run(["prefix", "repair", APP])
    R.equal("prefix repair: exit status", done.returncode, 0)
    R.check("prefix repair: Wine restored a deleted system file", victim.is_file())
    R.check("prefix repair: user files are kept", marker.is_file())
    R.check("prefix repair: Wine's own updating is still switched off",
            (prefix / ".update-timestamp").read_text() == "disable\n")
    done = bench.uos_run(bench.launch_arguments(profile, ["0"]))
    R.check("launch after repair: the run count continues", "prefix-runs=6" in tagged(done.stdout),
            str(tagged(done.stdout)))

    section("Hostile grants and metadata")
    for hostile in ["/", str(bench.home), str(bench.home / ".ssh"), str(bench.grant / ".." / "outside"),
                    "/proc/self/root", str(bench.data), str(bench.app_dir("local.other-app"))]:
        done = bench.uos_run(["prefix", "grant", APP, hostile, "--read-write"])
        R.check(f"grant of {hostile}: refused with UOS-SECURITY-SANDBOX-003",
                done.returncode == 6 and "error[UOS-SECURITY-SANDBOX-003]" in done.stderr, done.stderr)
    link = bench.grant / "link-to-outside"
    if not link.is_symlink():
        link.symlink_to(bench.outside)
    through_link = link / "secret.txt"
    R.check("the planted link leads to the secret on the host", through_link.read_text() == "outside-secret\n")
    done = bench.uos_run(bench.launch_arguments(profile, ["0"], also_outside=(through_link,)))
    lines = tagged(done.stdout)
    R.check("a link planted inside a grant does not lead out of the sandbox",
            f"outside-read: denied {through_link}" in lines and not any("LEAKED" in line for line in lines),
            str(lines))
    link.unlink()
    good = bench.metadata_path().read_text()
    edited = json.loads(good)
    edited["grants"].append({"path": str(bench.home / ".ssh"), "access": "read-only"})
    bench.metadata_path().write_text(json.dumps(edited, indent=2) + "\n")
    done = bench.uos_run(bench.launch_arguments(profile, ["0"]))
    R.equal("metadata edited to grant ~/.ssh: launch refused", done.returncode, 6)
    R.check("metadata edited to grant ~/.ssh: code UOS-SECURITY-SANDBOX-004",
            "error[UOS-SECURITY-SANDBOX-004]" in done.stderr, done.stderr)
    R.check("metadata edited to grant ~/.ssh: the program did not run", not tagged(done.stdout))
    bench.metadata_path().write_text(good)

    section("Root is refused")
    as_root = run(["sudo", "env", f"XDG_DATA_HOME={bench.work / 'root-data'}", f"XDG_STATE_HOME={bench.work / 'root-state'}",
                   bench.uos, "launch", profile, "--", "0"])
    R.equal("launch as root: refused", as_root.returncode, 6)
    R.check("launch as root: code UOS-SECURITY-SANDBOX-001", "error[UOS-SECURITY-SANDBOX-001]" in as_root.stderr,
            as_root.stderr)
    R.check("launch as root: the program did not run", not tagged(as_root.stdout))
    as_root = run(["sudo", "env", f"XDG_DATA_HOME={bench.work / 'root-data'}", f"XDG_STATE_HOME={bench.work / 'root-state'}",
                   bench.uos, "prefix", "create", "local.root-test"])
    R.equal("prefix create as root: refused", as_root.returncode, 6)
    R.check("as root: nothing was created", not (bench.work / "root-data").exists())


def check_gui(bench: Bench) -> None:
    section("GUI program on an X display, first launch creates the prefix")
    if not shutil.which("Xvfb") or not shutil.which("xwininfo"):
        R.check("GUI: Xvfb and xwininfo are installed", False, "install xvfb and x11-utils")
        return
    display = ":99"
    server = subprocess.Popen(["Xvfb", display, "-screen", "0", "1280x800x24", "-nolisten", "tcp", "-ac"],
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        socket = pathlib.Path("/tmp/.X11-unix/X99")
        for _ in range(50):
            if socket.exists():
                break
            time.sleep(0.2)
        R.check("GUI: the X server is up", socket.exists())
        profile = bench.profile("profile-gui.json", GUI_APP, "gui64.exe")
        command = [bench.uos, "launch", profile, "--env", f"UOS_TEST_GRANT_DIR={bench.grant}"]
        R.check("GUI: the application has no prefix yet", not bench.app_dir(GUI_APP).exists())
        # This first launch creates the prefix. Nothing is granted yet, so
        # the program cannot leave its report; the window is looked for on
        # the X display instead.
        process = subprocess.Popen(command, env=bench.env(DISPLAY=display), stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True, errors="replace")
        seen = False
        deadline = time.time() + 120
        while time.time() < deadline and process.poll() is None:
            tree = run(["xwininfo", "-display", display, "-root", "-tree"], timeout=20).stdout
            if "UOS test window" in tree:
                seen = True
                break
            time.sleep(0.25)
        try:
            out, err = process.communicate(timeout=180)
        except subprocess.TimeoutExpired:
            process.kill()
            out, err = process.communicate()
            err += "\n[timed out]"
        R.check("GUI: the program's window appeared on the X display", seen, tail(out + err))
        R.equal("GUI: the exit status is the program's", process.returncode, 7)
        R.check("GUI: the first launch created the prefix",
                (bench.app_dir(GUI_APP) / "prefix/system.reg").is_file()
                and bench.metadata_path(GUI_APP).is_file()
                and bench.metadata(GUI_APP).get("launch_count") == 1)
        marker = bench.grant / "gui-ran.txt"
        R.check("GUI: without a grant the program could not write to the directory", not marker.exists())
        # Granted, the same program leaves its report.
        done = bench.uos_run(["prefix", "grant", GUI_APP, str(bench.grant), "--read-write"])
        R.equal("GUI: prefix grant", done.returncode, 0)
        done = run(command, env=bench.env(DISPLAY=display), timeout=300)
        R.equal("GUI: second launch exit status", done.returncode, 7)
        text = marker.read_text() if marker.exists() else ""
        R.check("GUI: the program created a visible window", text.startswith("window-created visible=1"),
                text or tail(done.stdout + done.stderr))
        R.fact("GUI program report", text.strip())
        R.check("GUI: the two applications have separate prefixes",
                bench.app_dir(GUI_APP).is_dir() and bench.app_dir(APP).is_dir()
                and bench.metadata(GUI_APP).get("app_id") == GUI_APP)
    finally:
        server.terminate()
        try:
            server.wait(timeout=10)
        except subprocess.TimeoutExpired:
            server.kill()


def check_reset_and_remove(bench: Bench) -> None:
    section("Reset and remove")
    prefix = bench.app_dir() / "prefix"
    before = bench.metadata()
    done = bench.uos_run(["prefix", "reset", APP])
    R.equal("prefix reset: exit status", done.returncode, 0)
    after = bench.metadata() if bench.metadata_path().exists() else {}
    R.check("prefix reset: a fresh prefix", (prefix / "system.reg").is_file()
            and not (prefix / "drive_c/uos-test-runs.txt").exists())
    R.equal("prefix reset: the grants are kept", after.get("grants"), before.get("grants"))
    R.equal("prefix reset: the launch count starts again", after.get("launch_count"), 0)
    backups = sorted((bench.data / "uos/runtime/backups" / APP).glob("*-before-reset.tar.gz"))
    R.equal("prefix reset: the old prefix was archived first", len(backups), 1)

    # Removal deletes the application's logs and metadata; keep a copy as
    # evidence of what ran.
    kept = bench.work / "kept" / APP
    shutil.copytree(bench.state / "uos/runtime/logs" / APP, kept / "logs", dirs_exist_ok=True)
    shutil.copy(bench.metadata_path(), kept / "metadata.json")

    done = bench.uos_run(["prefix", "remove", APP])
    R.equal("prefix remove: exit status", done.returncode, 0)
    R.check("prefix remove: the application's logs are gone", not (bench.state / "uos/runtime/logs" / APP).exists())
    R.check("prefix remove: the application's directory is gone", not bench.app_dir().exists())
    R.check("prefix remove: backups are kept", bool(backups) and backups[0].is_file())
    R.check("prefix remove: the other applications are untouched",
            bench.other_file.is_file() and bench.app_dir(GUI_APP).is_dir())
    R.check("prefix remove: granted directories are untouched",
            (bench.grant / "written-by-windows.txt").is_file() and (bench.readonly / "readable.txt").is_file())
    R.check("the home directory canary is untouched", bench.key.read_text() == "home-key-canary\n")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--uos-runtime", required=True)
    parser.add_argument("--build-dir", required=True, type=pathlib.Path)
    parser.add_argument("--work-dir", required=True, type=pathlib.Path)
    parser.add_argument("--results", required=True, type=pathlib.Path)
    parser.add_argument("--only-inspect", action="store_true", help="skip everything that needs Wine")
    args = parser.parse_args()

    uos = str(pathlib.Path(args.uos_runtime).resolve())
    build = args.build_dir.resolve()
    work = args.work_dir.resolve()
    work.mkdir(parents=True, exist_ok=True)

    stages = [lambda: check_inspection(uos, build, work)]
    if not args.only_inspect:
        bench = Bench(uos, build, work)
        stages += [
            lambda: control_run(bench),
            lambda: check_prefix_and_launch(bench),
            lambda: check_gui(bench),
            lambda: check_reset_and_remove(bench),
        ]
    for stage in stages:
        try:
            stage()
        except Exception as error:  # a crash in the driver is a failed check, not a pass
            R.check(f"the test driver itself ({type(error).__name__})", False, repr(error))

    failed = R.failed
    args.results.write_text(json.dumps({"checks": R.checks, "facts": R.facts}, indent=2) + "\n")
    summary = [
        "## windows-runtime end-to-end",
        "",
        f"{len(R.checks) - len(failed)} of {len(R.checks)} checks passed.",
        "",
    ]
    summary += [f"- **{name}**: `{value}`" for name, value in R.facts.items()]
    if failed:
        summary += ["", "### Failed"] + [f"- {check['name']}" for check in failed]
    text = "\n".join(summary) + "\n"
    print("\n" + text)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as handle:
            handle.write(text)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
