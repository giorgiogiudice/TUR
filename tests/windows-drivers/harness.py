#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The windows-drivers experiment: Windows USB and HID drivers hosted by
Wine's user-space driver host, against virtual USB devices.

Runs as root (it grants and withdraws device access, as a TUR service would)
and runs every Wine process as an ordinary user (--user). Expects:

  * the virtual USB devices of virtual-usb/usbip_devices.py to be plugged in
    (1209:0001 loopback, 1209:0002 source/sink, 1209:0003 HID), and that
    program's hid-output.log at --hid-log;
  * the output of build.sh in --build;
  * wine, wineserver, setfacl, setpriv and bwrap on PATH.

Every scenario uses its own Wine prefix or says which one it reuses, writes
its logs under --evidence/scenarios/NN-name/, and records checks. A check is
a measurement, not a verdict on the harness: a FAIL that is the expected
negative result (for example "no device without a grant") is written as
such. The exit status is non-zero only when the harness itself could not
run (no gadgets, no build output, an exception); the results are in
results.json and results.md either way.

Nothing here loads anything into a kernel. The Windows drivers are loaded by
winedevice.exe, a Linux user-space process running as --user.
"""

from __future__ import annotations

import argparse
import glob
import gzip
import json
import os
import pathlib
import pwd
import re
import shlex
import shutil
import subprocess
import sys
import time
import traceback

VID = 0x1209
LOOPBACK, SOURCESINK, HIDPID = 0x0001, 0x0002, 0x0003

DEBUG_FULL = ("+wineusb,+plugplay,+setupapi,+usbd,+hid,+loaddll,warn+debugstr,"
              "+ntoskrnl,err+all,fixme+all")
DEBUG_QUIET = "-all"
# Creating a prefix installs all of wine.inf; +setupapi there is megabytes of
# noise and minutes of time.
DEBUG_INIT = "+wineusb,+plugplay,+loaddll,warn+debugstr,err+all"

EXCERPT_PATTERNS = [
    r"wineusb:", r"plugplay:", r"usbd:", r"AddDevice", r"Failed to", r"Unable to",
    r"Found compatible driver", r"err:", r"fixme:", r"twdusb:", r"kmdfprobe:", r"osrusbfx2",
    r"WDFLDR", r"Wdf01000", r"Unhandled", r"Loaded L\".*\.sys\"", r"load_driver", r"winebus:",
    r"hidraw", r"warn:debugstr",
]
# Lines that are noise for this experiment (no display on the runner).
EXCERPT_SKIP = [r"winediag:nodrv", r"explorer", r"systray", r"x11drv", r"wayland", r"vulkan",
                r"gecko", r"mono", r"fixme:(ole|shell|win|dwmapi|uxtheme|imm|msctf|ntdll|kernelbase|"
                r"wineboot|advapi|wbemprox|sxs|msi|setupapi:SetupDiInstallDevice|d3d|dxgi|wined3d|"
                r"user32|gdi|dbghelp|rpc|combase|propsys|xinput|mscoree|shdocvw|urlmon|wininet|"
                r"iphlpapi|nsi|netapi|mountmgr|winsock|httpapi|crypt|sspicli|secur32|kerberos|"
                r"msvcrt|ucrtbase|service|winsta|thread|sync|heap|virtual|module:load_builtin)"]


class HarnessError(Exception):
    pass


def now() -> float:
    return time.monotonic()


def write(path: pathlib.Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def read(path: pathlib.Path) -> str:
    try:
        return path.read_text(encoding="utf-8", errors="replace")
    except FileNotFoundError:
        return ""


# ---------------------------------------------------------------- devices

def gadget(pid: int) -> dict | None:
    """The host's view of a test gadget, from sysfs."""
    for d in sorted(glob.glob("/sys/bus/usb/devices/*")):
        try:
            if open(f"{d}/idVendor").read().strip() != f"{VID:04x}":
                continue
            if open(f"{d}/idProduct").read().strip() != f"{pid:04x}":
                continue
            bus = int(open(f"{d}/busnum").read())
            dev = int(open(f"{d}/devnum").read())
        except OSError:
            continue
        interfaces = []
        for i in sorted(glob.glob(f"{d}/{os.path.basename(d)}:*")):
            driver = os.path.basename(os.readlink(f"{i}/driver")) if os.path.islink(f"{i}/driver") else None
            hidraw = sorted("/dev/" + os.path.basename(h) for h in glob.glob(f"{i}/*/hidraw/hidraw*"))
            interfaces.append({"name": os.path.basename(i), "driver": driver, "hidraw": hidraw})
        return {"sysfs": os.path.realpath(d), "name": os.path.basename(d), "bus": bus, "dev": dev,
                "node": f"/dev/bus/usb/{bus:03d}/{dev:03d}", "interfaces": interfaces}
    return None


def need_gadget(pid: int, wait: float = 20.0) -> dict:
    deadline = now() + wait
    while True:
        g = gadget(pid)
        if g and os.path.exists(g["node"]):
            return g
        if now() > deadline:
            raise HarnessError(f"gadget {VID:04x}:{pid:04x} not present")
        time.sleep(0.5)


# ---------------------------------------------------------------- the harness

class Harness:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.build = pathlib.Path(args.build).resolve()
        self.evidence = pathlib.Path(args.evidence).resolve()
        self.evidence.mkdir(parents=True, exist_ok=True)
        entry = pwd.getpwnam(args.user)
        self.user, self.uid, self.gid, self.home = args.user, entry.pw_uid, entry.pw_gid, pathlib.Path(entry.pw_dir)
        self.prefixes = self.home / "twd-prefixes"
        self.results: list[dict] = []
        self.counter = 0
        self.granted: set[str] = set()
        self.hid_log = pathlib.Path(args.hid_log) if args.hid_log else None

    # -------------------------------------------------------- running as the user
    def env(self, prefix: pathlib.Path, debug: str) -> dict:
        return {
            "HOME": str(self.home), "USER": self.user, "LOGNAME": self.user,
            "PATH": "/usr/local/bin:/usr/bin:/bin", "LANG": "C.UTF-8",
            "WINEPREFIX": str(prefix), "WINEDEBUG": debug,
            # No Mono or Gecko download prompts; nothing is fetched.
            "WINEDLLOVERRIDES": "mscoree=;mshtml=;winemenubuilder.exe=d",
        }

    def as_user(self, argv: list[str], env: dict, out: pathlib.Path, err: pathlib.Path,
                timeout: float, background: bool = False):
        cmd = ["setpriv", f"--reuid={self.uid}", f"--regid={self.gid}", "--init-groups", "--",
               "env", "-i", *[f"{k}={v}" for k, v in env.items()], *argv]
        out.parent.mkdir(parents=True, exist_ok=True)
        with open(out, "ab") as o, open(err, "ab") as e:
            o.write(f"$ {shlex.join(argv)}\n".encode())
            o.flush()
            proc = subprocess.Popen(cmd, stdin=subprocess.DEVNULL, stdout=o, stderr=e, cwd="/")
        if background:
            return proc
        try:
            return proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
            with open(out, "ab") as o:
                o.write(f"(killed after {timeout} s)\n".encode())
            return "timeout"

    def wine(self, sc: "Scenario", name: str, args: list[str], debug: str = DEBUG_FULL,
             timeout: float = 180, background: bool = False):
        return self.as_user(["wine", *args], self.env(sc.prefix, debug), sc.dir / f"{name}.out",
                            sc.dir / f"{name}.err", timeout, background)

    def kill(self, sc: "Scenario") -> None:
        env = self.env(sc.prefix, DEBUG_QUIET)
        self.as_user(["wineserver", "-k"], env, sc.dir / "wineserver.out", sc.dir / "wineserver.err", 30)
        self.as_user(["wineserver", "-w"], env, sc.dir / "wineserver.out", sc.dir / "wineserver.err", 60)

    def boot(self, sc: "Scenario", name: str = "boot", debug: str = DEBUG_FULL, init: bool = False):
        """Start the prefix's wineserver, services and winedevice. Their debug
        output goes to NAME.err for as long as they run."""
        self.kill(sc)
        return self.wine(sc, name, ["wineboot", "-i" if init else "-u"], DEBUG_INIT if init else debug,
                         timeout=600)

    def winpath(self, path: pathlib.Path) -> str:
        return "Z:" + str(path).replace("/", "\\")

    # -------------------------------------------------------- grants
    def grant(self, node: str) -> None:
        subprocess.run(["setfacl", "-m", f"u:{self.user}:rw", node], check=True)
        self.granted.add(node)

    def revoke(self, node: str) -> None:
        subprocess.run(["setfacl", "-x", f"u:{self.user}", node], check=False)
        self.granted.discard(node)

    def revoke_all(self) -> None:
        for node in list(self.granted):
            self.revoke(node)

    def acl(self, node: str) -> str:
        return subprocess.run(["getfacl", "-p", node], capture_output=True, text=True).stdout

    # -------------------------------------------------------- scenarios
    def scenario(self, name: str, title: str, prefix: str | None = None) -> "Scenario":
        self.counter += 1
        sc = Scenario(self, f"{self.counter:02d}-{name}", title, self.prefixes / (prefix or name))
        self.results.append(sc.record)
        return sc


class Scenario:
    def __init__(self, h: Harness, name: str, title: str, prefix: pathlib.Path) -> None:
        self.h, self.name, self.title, self.prefix = h, name, title, prefix
        self.dir = h.evidence / "scenarios" / name
        self.dir.mkdir(parents=True, exist_ok=True)
        os.chown(self.dir, h.uid, h.gid)
        self.record = {"scenario": name, "title": title, "prefix": prefix.name, "checks": [],
                       "notes": [], "error": None}
        self.started = now()

    def check(self, name: str, ok: bool, detail: str = "", source: str = "harness",
              expected: bool | None = None) -> None:
        item = {"name": name, "result": "PASS" if ok else "FAIL", "detail": detail, "source": source}
        if expected is not None:
            item["expected"] = "PASS" if expected else "FAIL"
        self.record["checks"].append(item)
        print(f"[{self.name}] {item['result']} {name}: {detail}", flush=True)

    def note(self, text: str) -> None:
        self.record["notes"].append(text)
        print(f"[{self.name}] note: {text}", flush=True)

    def program_checks(self, out_name: str, prefix_word: str, expected: dict | None = None) -> dict:
        """Parse 'TWD check NAME PASS|FAIL detail' lines of a program's
        output into checks; return the metrics it printed."""
        text = read(self.dir / f"{out_name}.out")
        metrics = {}
        for line in text.splitlines():
            m = re.match(rf"^{prefix_word} check (\S+) (PASS|FAIL) ?(.*)$", line)
            if m:
                exp = (expected or {}).get(m.group(1))
                self.check(f"{out_name}:{m.group(1)}", m.group(2) == "PASS", m.group(3), source=out_name,
                           expected=exp)
            m = re.match(rf"^{prefix_word} metric (\S+) (\S+)$", line)
            if m:
                metrics[m.group(1)] = float(m.group(2))
        if metrics:
            self.record.setdefault("metrics", {}).update(metrics)
        return metrics

    def logs(self) -> str:
        return "\n".join(read(p) for p in sorted(self.dir.glob("*.err")))

    def finish(self) -> None:
        self.record["seconds"] = round(now() - self.started, 1)
        lines, seen = [], set()
        for path in sorted(self.dir.glob("*.err")) + sorted(self.dir.glob("*.out")):
            for line in read(path).splitlines():
                if not any(re.search(p, line) for p in EXCERPT_PATTERNS):
                    continue
                if any(re.search(p, line) for p in EXCERPT_SKIP):
                    continue
                key = re.sub(r"0x[0-9a-fA-F]+|\b[0-9a-f]{4,}\b|\d+", "N", line)
                if key in seen:
                    continue
                seen.add(key)
                lines.append(f"{path.name}: {line}")
        write(self.dir / "excerpt.txt", "\n".join(lines[:600]) + ("\n(cut at 600 lines)\n" if len(lines) > 600 else "\n"))
        self.record["excerpt"] = str((self.dir / "excerpt.txt").relative_to(self.h.evidence))


# ---------------------------------------------------------------- the scenarios

def bwrap_argv(h: Harness, sc: Scenario, devices: list[str]) -> list[str]:
    """A sandbox shaped like tur-runtime's (docs/architecture/runtime-manager.md,
    "What the application sees"), plus exactly the granted device nodes."""
    argv = ["bwrap", "--unshare-user", "--unshare-ipc", "--unshare-pid", "--unshare-uts",
            "--unshare-cgroup-try", "--disable-userns", "--die-with-parent", "--new-session",
            "--ro-bind", "/usr", "/usr", "--ro-bind", "/etc", "/etc"]
    for link in ("bin", "sbin", "lib", "lib64", "lib32"):
        target = os.readlink(f"/{link}") if os.path.islink(f"/{link}") else None
        if target:
            argv += ["--symlink", target, f"/{link}"]
        elif os.path.isdir(f"/{link}"):
            argv += ["--ro-bind", f"/{link}", f"/{link}"]
    argv += ["--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp"]
    for sysdir in ("/sys/block", "/sys/bus", "/sys/class", "/sys/dev", "/sys/devices"):
        argv += ["--ro-bind-try", sysdir, sysdir]
    argv += ["--bind", str(h.home), str(h.home), "--ro-bind", str(h.build), str(h.build),
             "--bind", str(sc.dir), str(sc.dir)]
    for node in devices:
        argv += ["--dev-bind", node, node]
    argv += ["--remount-ro", "/", "--chdir", str(h.home)]
    return argv


def run_in_sandbox(h: Harness, sc: Scenario, name: str, devices: list[str], script: str,
                   timeout: float, background: bool = False):
    write(sc.dir / f"{name}.sh", script)
    os.chown(sc.dir / f"{name}.sh", h.uid, h.gid)
    argv = bwrap_argv(h, sc, devices) + ["/bin/sh", str(sc.dir / f"{name}.sh")]
    write(sc.dir / f"{name}.bwrap.txt", shlex.join(argv) + "\n")
    env = h.env(sc.prefix, DEBUG_FULL)
    return h.as_user(argv, env, sc.dir / f"{name}.out", sc.dir / f"{name}.err", timeout, background)


def s_environment(h: Harness) -> None:
    sc = h.scenario("environment", "What the runner offers", prefix="none")
    info = {}
    for key, cmd in {
        "kernel": ["uname", "-r"],
        "os": ["sh", "-c", ". /etc/os-release; echo $PRETTY_NAME"],
        "wine": ["wine", "--version"],
        "bwrap": ["bwrap", "--version"],
        "libusb": ["sh", "-c", "rpm -q libusb1 2>/dev/null || dpkg-query -W libusb-1.0-0 2>/dev/null"],
        "wine-package": ["sh", "-c", "rpm -q wine-core wine 2>/dev/null || dpkg-query -W wine wine64 libwine 2>/dev/null"],
    }.items():
        r = subprocess.run(cmd, capture_output=True, text=True)
        info[key] = (r.stdout.strip() or r.stderr.strip())
    info["wine-drivers"] = sorted({os.path.basename(p) for p in
                                   glob.glob("/usr/lib*/**/x86_64-windows/*.sys", recursive=True)})
    info["wine-has-wdfldr"] = any("wdfldr" in d.lower() or "wdf01000" in d.lower() for d in info["wine-drivers"])
    for pid in (LOOPBACK, SOURCESINK, HIDPID):
        info[f"gadget-{pid:04x}"] = gadget(pid)
    sc.record["environment"] = info
    write(sc.dir / "environment.json", json.dumps(info, indent=2) + "\n")
    for pid in (LOOPBACK, SOURCESINK, HIDPID):
        g = info[f"gadget-{pid:04x}"]
        sc.check(f"gadget-{pid:04x}-present", g is not None, json.dumps(g))
    sc.check("wine-ships-kmdf-loader", info["wine-has-wdfldr"], "a wdfldr.sys or wdf01000.sys among Wine's drivers",
             expected=False)
    sc.finish()


def s_install_without_grant(h: Harness) -> None:
    sc = h.scenario("install-no-grant", "Stage the INF; no device access granted", prefix="A")
    h.revoke_all()
    rc = h.boot(sc, "boot-init", init=True)
    sc.check("prefix-created", rc == 0, f"wineboot -i exit {rc}")
    h.wine(sc, "twdinst-copy", [str(h.build / "twdinst.exe"), "copy", h.winpath(h.build / "twdusb.inf")])
    out = read(sc.dir / "twdinst-copy.out")
    sc.check("SetupCopyOEMInf", "ok=1" in out, " ".join(l for l in out.splitlines() if "SetupCopyOEMInf" in l))
    h.wine(sc, "twdinst-files", [str(h.build / "twdinst.exe"), "files"])
    h.boot(sc, "boot")
    time.sleep(3)
    h.wine(sc, "twdinst-devices", [str(h.build / "twdinst.exe"), "devices"])
    devices = read(sc.dir / "twdinst-devices.out")
    sc.check("no-device-without-grant", "VID_1209" not in devices,
             "Wine's PnP manager lists no 1209 device when the user may not open it", expected=True)
    rc = h.wine(sc, "twdtest-info", [str(h.build / "twdtest.exe"), "info", "1209", "0001", "5"])
    sc.check("twdtest-finds-nothing", rc == 3, f"exit {rc}", expected=True)
    logs = sc.logs()
    sc.check("wineusb-could-not-open", "Failed to open device" in logs,
             "wineusb's WARN 'Failed to open device' for nodes it may not open (needs +wineusb)")
    h.kill(sc)
    sc.finish()


def s_wdm(h: Harness, pid: int, mode: str, name: str, title: str, debug: str = DEBUG_FULL) -> None:
    sc = h.scenario(name, title, prefix="A")
    g = need_gadget(pid)
    h.grant(g["node"])
    write(sc.dir / "grant.txt", json.dumps(g, indent=2) + "\n" + h.acl(g["node"]))
    h.boot(sc, "boot", debug)
    rc = h.wine(sc, f"twdtest-{mode}", [str(h.build / "twdtest.exe"), mode, "1209", f"{pid:04x}", "60"], debug,
                timeout=600)
    sc.note(f"twdtest {mode} exit {rc}")
    sc.program_checks(f"twdtest-{mode}", "TWD")
    h.wine(sc, "twdinst-devices", [str(h.build / "twdinst.exe"), "devices"], DEBUG_QUIET)
    if debug != DEBUG_QUIET:
        logs = sc.logs()
        sc.check("driver-entry-ran", "twdusb: DriverEntry" in logs, "twdusb.sys's DbgPrint from DriverEntry, in Wine's log")
        sc.check("add-device-ran", "twdusb: AddDevice" in logs, "twdusb.sys's AddDevice was called by Wine's PnP manager")
    h.kill(sc)
    sc.finish()


def s_winusb(h: Harness) -> None:
    sc = h.scenario("winusb", "A WinUSB-based program against Wine's winusb.dll", prefix="A")
    if not (h.build / "winusbtest.exe").exists():
        sc.check("built", False, "winusbtest.exe was not built")
        sc.finish()
        return
    g = need_gadget(LOOPBACK)
    h.grant(g["node"])
    h.boot(sc, "boot")
    rc = h.wine(sc, "winusbtest", [str(h.build / "winusbtest.exe"), "60"], timeout=180)
    sc.note(f"winusbtest exit {rc}")
    sc.program_checks("winusbtest", "WINUSB")
    err = read(sc.dir / "winusbtest.err")
    stub = [l for l in err.splitlines() if "unimplemented function" in l or "winusb" in l.lower()]
    sc.check("winusb-implemented", rc == 0, " | ".join(stub[:3])[:500], expected=False)
    h.kill(sc)
    sc.finish()


def s_sandbox(h: Harness) -> None:
    sc = h.scenario("sandbox-one-device", "In a tur-runtime-shaped bubblewrap sandbox with one device", prefix="A")
    loop, ss = need_gadget(LOOPBACK), need_gadget(SOURCESINK)
    # Both are allowed by ACL: only the sandbox decides which one is visible.
    h.grant(loop["node"])
    h.grant(ss["node"])
    h.kill(sc)
    script = f"""set -u
echo "== /dev/bus/usb inside the sandbox"; find /dev/bus -print 2>&1
echo "== hidraw inside the sandbox"; ls -l /dev/hidraw* 2>&1
wine wineboot -u >"{sc.dir}/sb-boot.out" 2>"{sc.dir}/sb-boot.err"
wine "{h.build}/twdtest.exe" loopback 1209 0001 60 >"{sc.dir}/twdtest-loopback.out" 2>"{sc.dir}/twdtest-loopback.err"
echo "loopback exit $?"
wine "{h.build}/twdtest.exe" info 1209 0002 15 >"{sc.dir}/twdtest-info-ss.out" 2>"{sc.dir}/twdtest-info-ss.err"
echo "sourcesink exit $?"
wineserver -k; wineserver -w
"""
    rc = run_in_sandbox(h, sc, "sandbox", [loop["node"]], script, timeout=600)
    out = read(sc.dir / "sandbox.out")
    sc.note(f"bwrap exit {rc}")
    sc.check("sandbox-ran", rc == 0 and "loopback exit" in out, f"bwrap exit {rc}; {read(sc.dir / 'sandbox.err')[-300:]}")
    nodes = [l for l in out.splitlines() if re.match(r"^/dev/bus/usb/\d+/\d+$", l)]
    sc.check("only-granted-node-inside", nodes == [loop["node"]], f"USB device nodes inside: {nodes}")
    sc.program_checks("twdtest-loopback", "TWD")
    sc.check("other-device-invisible", "sourcesink exit 3" in out,
             "the source/sink gadget, allowed by ACL but not bound into the sandbox, is not found", expected=True)
    sc.finish()


def s_revoke(h: Harness) -> None:
    sc = h.scenario("revoke-while-running", "Withdraw the device while the application uses it", prefix="A")
    loop = need_gadget(LOOPBACK)
    h.grant(loop["node"])
    h.kill(sc)
    script = f"""set -u
wine wineboot -u >"{sc.dir}/sb-boot.out" 2>"{sc.dir}/sb-boot.err"
wine "{h.build}/twdtest.exe" soak 1209 0001 60 40 >"{sc.dir}/twdtest-soak.out" 2>"{sc.dir}/twdtest-soak.err"
echo "soak exit $?"
wineserver -k; wineserver -w
"""
    proc = run_in_sandbox(h, sc, "sandbox", [loop["node"]], script, timeout=0, background=True)
    # Wait for the soak to be running.
    deadline = now() + 120
    while now() < deadline and "check device-found PASS" not in read(sc.dir / "twdtest-soak.out"):
        time.sleep(1)
    time.sleep(5)
    # The revocation a TUR service would do: take the ACL away (that does
    # nothing to a file already open) and disconnect the device at its hub
    # port, which ends every open usbfs file of it. If the port cannot be
    # disabled or the device stays, a logical disconnect through sysfs.
    h.revoke(loop["node"])
    parent = os.path.dirname(loop["sysfs"])
    portnum = loop["name"].split("-")[-1].split(".")[-1]
    candidates = glob.glob(f"{parent}/*/*-port{portnum}/disable") + glob.glob(f"{parent}/*-port{portnum}/disable")
    t_revoke = now()
    method = None

    def gone() -> bool:
        g = gadget(LOOPBACK)
        return g is None or g["dev"] != loop["dev"]

    if candidates:
        try:
            write(pathlib.Path(candidates[0]), "1")
            deadline = now() + 15
            while now() < deadline and not gone():
                time.sleep(0.5)
            if gone():
                method = f"hub port disabled ({candidates[0]})"
            else:
                sc.note(f"port {candidates[0]} disabled, but the device stayed")
        except OSError as exc:
            sc.note(f"port disable failed: {exc}")
    if not method:
        try:
            write(pathlib.Path(loop["sysfs"]) / "remove", "1")
            method = "logical disconnect (sysfs remove)"
        except OSError as exc:
            sc.note(f"sysfs remove failed: {exc}")
            method = "none worked"
    sc.note(f"revoked by: {method}")
    try:
        rc = proc.wait(timeout=180)
    except subprocess.TimeoutExpired:
        proc.kill()
        rc = "timeout"
    took = now() - t_revoke
    out = read(sc.dir / "twdtest-soak.out")
    sc.program_checks("twdtest-soak", "TWD")
    sc.check("application-saw-revocation", "check soak FAIL" in out,
             "the soak stopped: the application lost the device", expected=True)
    sc.check("application-not-hung", rc != "timeout", f"sandbox exit {rc}, {took:.1f} s after revocation")
    sc.note(f"device afterwards: {json.dumps(gadget(LOOPBACK))}")
    sc.finish()


def s_pnputil(h: Harness) -> None:
    sc = h.scenario("pnputil-diinstall", "Install with pnputil and DiInstallDriver instead", prefix="B")
    loop = need_gadget(LOOPBACK)
    h.grant(loop["node"])
    h.boot(sc, "boot-init", init=True)
    h.wine(sc, "pnputil", ["pnputil", "/add-driver", h.winpath(h.build / "twdusb.inf"), "/install"])
    h.wine(sc, "twdinst-diinstall", [str(h.build / "twdinst.exe"), "diinstall", h.winpath(h.build / "twdusb.inf")])
    h.wine(sc, "twdinst-files", [str(h.build / "twdinst.exe"), "files"])
    files = read(sc.dir / "twdinst-files.out")
    # Wine's SetupCopyOEMInf keeps the INF's name (oemN.inf only on a clash)
    # and puts the package in the driver store as NAME.inf_N.
    staged = "TWDINST file oem" in files or "twdusb.inf_" in files
    sc.check("inf-staged", staged, "the package in C:\\windows\\inf or the driver store after pnputil and "
             "DiInstallDriverW", expected=False)
    logs = sc.logs()
    sc.check("pnputil-is-stub", "fixme:pnputil" in logs or "stub" in read(sc.dir / "pnputil.err"),
             "Wine's pnputil.exe prints a FIXME and does nothing")
    sc.check("DiInstallDriver-is-stub", "DiInstallDriverW" in logs and "stub" in logs,
             "Wine's DiInstallDriverW prints a FIXME and returns TRUE")
    h.boot(sc, "boot")
    rc = h.wine(sc, "twdtest-info", [str(h.build / "twdtest.exe"), "info", "1209", "0001", "15"])
    sc.check("driver-bound", rc == 0, f"twdtest info exit {rc}", expected=False)
    h.kill(sc)
    sc.finish()


def s_kmdf(h: Harness, sys_name: str, inf_name: str, name: str, title: str) -> None:
    sc = h.scenario(name, title, prefix=name)
    if not (h.build / sys_name).exists():
        sc.check("built", False, f"{sys_name} was not built; see the build log")
        sc.finish()
        return
    loop = need_gadget(LOOPBACK)
    h.grant(loop["node"])
    h.boot(sc, "boot-init", init=True)
    h.wine(sc, "twdinst-copy", [str(h.build / "twdinst.exe"), "copy", h.winpath(h.build / inf_name)])
    sc.check("SetupCopyOEMInf", "ok=1" in read(sc.dir / "twdinst-copy.out"), read(sc.dir / "twdinst-copy.out").strip()[-300:])
    h.boot(sc, "boot")
    time.sleep(10)
    h.wine(sc, "twdinst-devices", [str(h.build / "twdinst.exe"), "devices"], DEBUG_QUIET)
    h.wine(sc, "twdinst-files", [str(h.build / "twdinst.exe"), "files"], DEBUG_QUIET)
    logs = sc.logs()
    stem = sys_name.split(".")[0]
    matched = [l for l in logs.splitlines() if "WDFLDR" in l.upper() and ("not found" in l or "err:" in l)]
    load_failed = [l for l in logs.splitlines()
                   if ("Failed to load driver" in l or "failed to create driver" in l) and stem in l]
    sc.check("service-installed", stem in read(sc.dir / "twdinst-devices.out"),
             "Wine's PnP manager installed the package for the device (service name in the device list)")
    sc.check("driver-entry-ran", f"{stem}: DriverEntry" in logs or "osrusbfx2: DriverEntry" in logs,
             "the driver's own DriverEntry ran", expected=False)
    sc.check("wdfldr-missing-reported", bool(matched), " | ".join(matched[:3])[:600])
    sc.check("load-failure-reported", bool(load_failed), " | ".join(load_failed[:3])[:600])
    h.kill(sc)
    sc.finish()


def s_hid(h: Harness) -> None:
    sc = h.scenario("hid-winebus", "HID gadget through hidraw, winebus.sys and hidclass.sys to hid.dll", prefix="E")
    g = need_gadget(HIDPID)
    hidraw = [n for i in g["interfaces"] for n in i["hidraw"]]
    sc.note(f"HID gadget: {json.dumps(g)}")
    if not hidraw:
        sc.check("hidraw-node", False, "the host made no hidraw node for the HID gadget")
        sc.finish()
        return
    h.revoke_all()
    h.boot(sc, "boot-init", init=True)
    rc = h.wine(sc, "hidtest-no-grant", [str(h.build / "hidtest.exe"), "1209", "0003", "8"])
    sc.check("no-hid-without-grant", rc == 3, f"hidtest exit {rc} without access to {hidraw[0]}", expected=True)
    h.grant(hidraw[0])
    write(sc.dir / "grant.txt", h.acl(hidraw[0]))
    h.boot(sc, "boot")
    before = len(read(h.hid_log).splitlines()) if h.hid_log else 0
    rc = h.wine(sc, "hidtest", [str(h.build / "hidtest.exe"), "1209", "0003", "30"], timeout=180)
    sc.note(f"hidtest exit {rc}")
    sc.program_checks("hidtest", "HID")
    time.sleep(1)
    received = read(h.hid_log).splitlines()[before:] if h.hid_log else []
    got = [line for line in received if "TUROUT" in line]
    sc.check("output-report-delivered", bool(got),
             f"the device received {len(received)} output reports; first with TUROUT: {got[0] if got else '-'}")
    h.kill(sc)
    h.revoke(hidraw[0])
    sc.finish()


def s_conflict(h: Harness) -> None:
    sc = h.scenario("raw-usb-vs-linux-driver",
                    "A Windows USB driver for a device a Linux driver (usbhid) already has", prefix="A")
    g = need_gadget(HIDPID)
    iface = g["interfaces"][0]["name"] if g["interfaces"] else None
    sc.note(f"interfaces before: {json.dumps(g['interfaces'])}")
    h.grant(g["node"])
    h.boot(sc, "boot-bound")
    rc = h.wine(sc, "twdtest-bound", [str(h.build / "twdtest.exe"), "interrupt", "1209", "0003", "60"], timeout=300)
    sc.note(f"with usbhid bound: twdtest exit {rc}")
    sc.program_checks("twdtest-bound", "TWD")
    h.kill(sc)
    # Detach the Linux driver from the interface, as a TUR grant for a
    # device with a Linux driver would have to.
    if iface and g["interfaces"][0]["driver"]:
        driver = g["interfaces"][0]["driver"]
        write(pathlib.Path(f"/sys/bus/usb/drivers/{driver}/unbind"), iface)
        sc.note(f"unbound {iface} from {driver}")
        time.sleep(1)
        h.boot(sc, "boot-unbound")
        rc = h.wine(sc, "twdtest-unbound", [str(h.build / "twdtest.exe"), "interrupt", "1209", "0003", "60"],
                    timeout=300)
        sc.note(f"with the interface unbound: twdtest exit {rc}")
        sc.program_checks("twdtest-unbound", "TWD")
        h.kill(sc)
        write(pathlib.Path(f"/sys/bus/usb/drivers/{driver}/bind"), iface)
        sc.note(f"rebound {iface} to {driver}")
    h.revoke(g["node"])
    sc.finish()


def markdown(h: Harness, label: str) -> str:
    out = [f"## Windows drivers in Wine's user-space driver host: {label}", ""]
    env = next((r.get("environment") for r in h.results if r.get("environment")), {})
    if env:
        out += ["| | |", "|---|---|"]
        for key in ("os", "kernel", "wine", "wine-package", "libusb", "bwrap", "wine-has-wdfldr"):
            out.append(f"| {key} | {str(env.get(key, '')).replace('|', '/')} |")
        out.append("")
    for r in h.results:
        out += [f"### {r['scenario']}: {r['title']}", ""]
        if r.get("error"):
            out += ["**Harness error:**", "```", r["error"][-2000:], "```", ""]
        if r["checks"]:
            out += ["| check | result | expected | detail |", "|---|---|---|---|"]
            for c in r["checks"]:
                detail = c["detail"].replace("|", "/").replace("\n", " ")[:300]
                out.append(f"| {c['name']} | {c['result']} | {c.get('expected', '')} | {detail} |")
            out.append("")
        for n in r["notes"]:
            out.append(f"- {n}")
        if r.get("metrics"):
            out.append(f"- metrics: {json.dumps(r['metrics'])}")
        out.append("")
    return "\n".join(out) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--build", required=True)
    ap.add_argument("--evidence", required=True)
    ap.add_argument("--user", default="tur")
    ap.add_argument("--label", default="")
    ap.add_argument("--hid-log", default="", help="the virtual HID device's record of output reports")
    ap.add_argument("--only", default="", help="comma-separated scenario functions to run")
    args = ap.parse_args()
    if os.geteuid() != 0:
        print("run as root: the harness grants device access as a TUR service would", file=sys.stderr)
        return 2
    h = Harness(args)
    h.prefixes.mkdir(parents=True, exist_ok=True)
    os.chown(h.prefixes, h.uid, h.gid)
    for required in ("twdusb.sys", "twdinst.exe", "twdtest.exe", "hidtest.exe", "twdusb.inf"):
        if not (h.build / required).exists():
            print(f"missing build output: {required}", file=sys.stderr)
            return 2

    plan = [
        ("environment", lambda: s_environment(h)),
        ("install", lambda: s_install_without_grant(h)),
        ("loopback", lambda: s_wdm(h, LOOPBACK, "loopback", "wdm-loopback", "twdusb.sys on the loopback gadget: bulk transfers")),
        ("loopback-quiet", lambda: s_wdm(h, LOOPBACK, "loopback", "wdm-loopback-quiet",
                                         "The same without Wine debug output, for timings", DEBUG_QUIET)),
        ("sourcesink", lambda: s_wdm(h, SOURCESINK, "sourcesink", "wdm-sourcesink",
                                     "twdusb.sys on the source/sink gadget: vendor control and patterned bulk")),
        ("winusb", lambda: s_winusb(h)),
        ("sandbox", lambda: s_sandbox(h)),
        ("pnputil", lambda: s_pnputil(h)),
        ("kmdfprobe", lambda: s_kmdf(h, "kmdfprobe.sys", "kmdfprobe.inf", "kmdf-probe",
                                     "A driver that binds to KMDF like every KMDF driver")),
        ("kmdffx2", lambda: s_kmdf(h, "osrusbfx2.sys", "osrusbfx2.inf", "kmdf-fx2",
                                   "Microsoft's kmdf_fx2 sample, built with mingw-w64")),
        ("hid", lambda: s_hid(h)),
        ("conflict", lambda: s_conflict(h)),
        ("revoke", lambda: s_revoke(h)),
    ]
    only = set(filter(None, args.only.split(",")))
    harness_failed = False
    for key, fn in plan:
        if only and key not in only:
            continue
        print(f"===== {key}", flush=True)
        try:
            fn()
        except Exception:
            text = traceback.format_exc()
            print(text, file=sys.stderr, flush=True)
            if h.results:
                h.results[-1]["error"] = text
            harness_failed = True
    h.revoke_all()

    write(h.evidence / "results.json", json.dumps({"label": args.label, "scenarios": h.results}, indent=2) + "\n")
    write(h.evidence / "results.md", markdown(h, args.label))
    # Full logs compressed; excerpts stay readable.
    for path in h.evidence.glob("scenarios/*/*.err"):
        if path.stat().st_size > 256 * 1024:
            with open(path, "rb") as src, gzip.open(f"{path}.gz", "wb") as dst:
                shutil.copyfileobj(src, dst)
            path.unlink()
    print(read(h.evidence / "results.md"))
    return 1 if harness_failed else 0


if __name__ == "__main__":
    sys.exit(main())
