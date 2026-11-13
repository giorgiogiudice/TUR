#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The KMDF half of the windows-drivers experiment (windows-drivers-2):
KMDF drivers as user-space programs in Wine's driver host, with the KMDF
runtime built from Microsoft's MIT-licensed source and Wine's ntoskrnl.exe
plus a new wdfldr.sys built with TUR's patch set.

Runs as root inside the Fedora 45 container (it grants device access and
installs the Wine overlay into the throw-away container's Wine), and runs
every Wine process as an ordinary user. Expects the virtual USB devices
1209:0001 (loopback) and 0547:1002 (FX2-like) and, in --build, what
ci-build.sh produced: Wdf01000.sys, ntoskrnl.exe and wdfldr.sys (the
overlay), kmdftest.sys/.inf/.exe, osrusbfx2.sys/.inf/.exe, and the
twdinst.exe of the first experiment (build.sh).

Scenarios (each its own Wine prefix):
  overlay         put the patched ntoskrnl.exe and the new wdfldr.sys over
                  Fedora's Wine; record versions and hashes
  kmdftest        the minimal KMDF driver on the loopback device: binding,
                  WdfDriverCreate, EvtDriverDeviceAdd, EvtDevicePrepareHardware,
                  EvtDeviceD0Entry, WDFDPC/WDFTIMER/WDFWORKITEM, bulk and vendor
                  transfers through the framework
  fx2             Microsoft's kmdf_fx2 sample on the FX2-like device, driven
                  by the sample's own test program

Writes results.json and results.md under --evidence; the exit status is
non-zero only when the harness itself failed. Nothing here loads anything
into a kernel.
"""

from __future__ import annotations

import argparse
import glob
import gzip
import hashlib
import importlib.util
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time
import traceback

HERE = pathlib.Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("twd_harness", HERE.parent / "harness.py")
twd = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(twd)

DEBUG_KMDF = ("+wdfldr,+plugplay,+loaddll,+wineusb,+usbd,+seh,warn+debugstr,err+all,fixme+all")
FX2_VID, FX2_PID = 0x0547, 0x1002
twd.EXCERPT_PATTERNS += [r"wdfldr", r"WDF\[", r"kmdftest:", r"fxstub", r"OsrFx", r"osrusbfx2", r"Wdf01000",
                         r"seh:", r"Unhandled exception", r"unimplemented function", r"page fault"]


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fx2_gadget(wait: float = 20.0) -> dict:
    deadline = time.monotonic() + wait
    while True:
        for d in sorted(glob.glob("/sys/bus/usb/devices/*")):
            try:
                if open(f"{d}/idVendor").read().strip() != f"{FX2_VID:04x}":
                    continue
                if open(f"{d}/idProduct").read().strip() != f"{FX2_PID:04x}":
                    continue
                bus, dev = int(open(f"{d}/busnum").read()), int(open(f"{d}/devnum").read())
            except OSError:
                continue
            node = f"/dev/bus/usb/{bus:03d}/{dev:03d}"
            if os.path.exists(node):
                return {"sysfs": os.path.realpath(d), "name": os.path.basename(d), "node": node}
        if time.monotonic() > deadline:
            raise twd.HarnessError("FX2-like device 0547:1002 not present")
        time.sleep(0.5)


def wine_pe_dir() -> pathlib.Path:
    hits = sorted(glob.glob("/usr/lib*/**/x86_64-windows/ntoskrnl.exe", recursive=True))
    if not hits:
        raise twd.HarnessError("Wine's x86_64-windows directory not found")
    return pathlib.Path(hits[0]).parent


class Kmdf:
    def __init__(self, h, build: pathlib.Path) -> None:
        self.h, self.build = h, build

    def wine_input(self, sc, name: str, args: list[str], stdin: bytes, timeout: float, debug: str = DEBUG_KMDF):
        """Like Harness.wine, with bytes for the program's standard input."""
        env = self.h.env(sc.prefix, debug)
        cmd = ["setpriv", f"--reuid={self.h.uid}", f"--regid={self.h.gid}", "--init-groups", "--",
               "env", "-i", *[f"{k}={v}" for k, v in env.items()], "wine", *args]
        out, err = sc.dir / f"{name}.out", sc.dir / f"{name}.err"
        with open(out, "ab") as o, open(err, "ab") as e:
            o.write(("$ wine " + " ".join(args) + "\n").encode())
            o.flush()
            try:
                return subprocess.run(cmd, input=stdin, stdout=o, stderr=e, cwd="/", timeout=timeout).returncode
            except subprocess.TimeoutExpired:
                o.write(f"(killed after {timeout} s)\n".encode())
                return "timeout"

    def install_runtime(self, sc) -> None:
        """Wdf01000.sys into system32\\drivers and the Wdf01000 kernel service
        the loader starts (on Windows the KMDF co-installer or the OS image
        provides both)."""
        drivers = sc.prefix / "drive_c" / "windows" / "system32" / "drivers"
        drivers.mkdir(parents=True, exist_ok=True)
        shutil.copy2(self.build / "Wdf01000.sys", drivers / "Wdf01000.sys")
        os.chown(drivers / "Wdf01000.sys", self.h.uid, self.h.gid)
        rc = self.h.wine(sc, "sc-create", ["sc", "create", "Wdf01000", "type=", "kernel", "start=", "demand",
                                           "binpath=", "C:\\windows\\system32\\drivers\\Wdf01000.sys"],
                         twd.DEBUG_QUIET)
        sc.check("runtime-service-created", rc == 0 and "SUCCESS" in twd.read(sc.dir / "sc-create.out").upper()
                 or rc == 0, f"sc create Wdf01000 exit {rc}: {twd.read(sc.dir / 'sc-create.out').strip()[-200:]}")
        # The framework's own debug prints (version.cpp's __Print) go to
        # DbgPrintEx when this diagnostics value is set; TurTraceLevel 5 also
        # turns on the framework's verbose trace messages.
        self.h.wine(sc, "reg-diag", ["reg", "add", "HKLM\\System\\CurrentControlSet\\Control\\Wdf\\Kmdf\\Diagnostics",
                                     "/v", "DbgPrintOn", "/t", "REG_DWORD", "/d", "1", "/f"], twd.DEBUG_QUIET)

    def install_package(self, sc, inf: str) -> None:
        self.h.wine(sc, "twdinst-copy", [str(self.build / "twdinst.exe"), "copy", self.h.winpath(self.build / inf)])
        out = twd.read(sc.dir / "twdinst-copy.out")
        sc.check("SetupCopyOEMInf", "ok=1" in out, out.strip()[-300:])

    def common_checks(self, sc, stem: str) -> str:
        logs = sc.logs()
        lines = logs.splitlines()

        def first(pattern: str) -> str:
            for line in lines:
                if re.search(pattern, line):
                    return line.strip()[:400]
            return ""

        bind = first(r"TUR fxstub: WdfVersionBind returned")
        sc.check("client-image-loaded", bool(first(r"TUR fxstub: binding")),
                 first(r"TUR fxstub: binding") or "the KMDF stub's first message is not in the log "
                 + (first(r"err:module:import_dll.*" + stem) or first(r"Failed to load driver.*" + stem)))
        sc.check("wdfldr-loaded", bool(first(r"[Ll]oaded.*wdfldr\.sys")), first(r"[Ll]oaded.*wdfldr\.sys"))
        sc.check("framework-library-loaded", bool(first(r"[Ll]oaded.*Wdf01000\.sys")) or
                 bool(first(r"wdfldr:.*WdfRegisterLibrary")), first(r"[Ll]oaded.*Wdf01000\.sys")
                 or first(r"wdfldr:.*ZwLoadDriver") or first(r"wdfldr:.*err") or "not loaded")
        sc.check("library-registered", bool(first(r"WdfRegisterLibrary|library version 1\.33")),
                 first(r"library version") or first(r"framework library did not register"))
        sc.check("version-bind-succeeded", "returned 0x00000000" in bind, bind or "no WdfVersionBind result")
        sc.check("driver-entry-succeeded", bool(first(r"TUR fxstub: DriverEntry returned 0x00000000")),
                 first(r"TUR fxstub: DriverEntry returned") or "DriverEntry did not return")
        crash = first(r"Unhandled exception|unimplemented function|page fault on|wine: Assertion|KeBugCheck")
        sc.check("no-driver-host-crash", not crash, crash or "no unhandled exception or unimplemented call logged")
        stubs = sorted({m.group(1) for m in re.finditer(r"unimplemented function ([\w.]+)", logs)})
        if stubs:
            sc.note("unimplemented functions called: " + ", ".join(stubs))
        fixmes = sorted({re.sub(r"0x[0-9a-f]+|\d+|[0-9a-f]{6,}", "N", m.group(0))[:160]
                         for m in re.finditer(r"fixme:(ntoskrnl|wdfldr|wineusb|usbd|plugplay|hal)[^\n]*", logs)})
        if fixmes:
            twd.write(sc.dir / "fixmes.txt", "\n".join(fixmes) + "\n")
            sc.note(f"{len(fixmes)} distinct FIXMEs from the kernel modules (fixmes.txt)")
        trace = [l for l in lines if "WDF[" in l]
        twd.write(sc.dir / "wdf-trace.txt", "\n".join(trace) + "\n")
        return logs


def s_overlay(h, build: pathlib.Path) -> None:
    sc = h.scenario("kmdf-overlay", "Wine 11.20 + TUR patch set over Fedora's Wine; the KMDF runtime", prefix="none")
    pe = wine_pe_dir()
    info = {"wine-pe-dir": str(pe)}
    for name in ("ntoskrnl.exe", "wdfldr.sys"):
        src = build / name
        if not src.exists():
            sc.check(f"overlay-{name}", False, "not built")
            continue
        before = sha256(pe / name) if (pe / name).exists() else None
        shutil.copy2(src, pe / name)
        info[name] = {"fedora-sha256": before, "tur-sha256": sha256(pe / name), "bytes": (pe / name).stat().st_size}
        sc.check(f"overlay-{name}", sha256(pe / name) == sha256(src),
                 f"{pe / name}: Fedora's {before and before[:12]} replaced by {sha256(src)[:12]}" if before
                 else f"{pe / name}: new")
    for name in ("Wdf01000.sys", "kmdftest.sys", "osrusbfx2.sys", "kmdftest.exe", "osrusbfx2.exe", "twdinst.exe"):
        p = build / name
        info[name] = {"sha256": sha256(p), "bytes": p.stat().st_size} if p.exists() else None
        sc.check(f"built-{name}", p.exists(), f"{p.stat().st_size} bytes" if p.exists() else "missing")
    r = subprocess.run(["wine", "--version"], capture_output=True, text=True)
    info["wine"] = r.stdout.strip()
    sc.record["environment"] = info
    twd.write(sc.dir / "overlay.json", json.dumps(info, indent=2) + "\n")
    sc.finish()


def s_kmdftest(h, k: Kmdf) -> None:
    sc = h.scenario("kmdftest-loopback", "The minimal KMDF driver on the loopback device", prefix="kmdftest")
    if not (k.build / "kmdftest.sys").exists() or not (k.build / "Wdf01000.sys").exists():
        sc.check("built", False, "kmdftest.sys or Wdf01000.sys was not built")
        sc.finish()
        return
    g = twd.need_gadget(twd.LOOPBACK)
    h.grant(g["node"])
    h.boot(sc, "boot-init", init=True)
    k.install_runtime(sc)
    k.install_package(sc, "kmdftest.inf")
    h.boot(sc, "boot", DEBUG_KMDF)
    time.sleep(5)
    rc = h.wine(sc, "kmdftest", [str(k.build / "kmdftest.exe"), "30"], DEBUG_KMDF, timeout=240)
    sc.note(f"kmdftest.exe exit {rc}")
    sc.program_checks("kmdftest", "KMDF")
    h.wine(sc, "twdinst-devices", [str(k.build / "twdinst.exe"), "devices"], twd.DEBUG_QUIET)
    logs = k.common_checks(sc, "kmdftest")
    for step in ("DriverEntry", "WdfDriverCreate returned 0x00000000", "EvtDriverDeviceAdd",
                 "WdfDeviceCreate returned 0x00000000", "EvtDevicePrepareHardware", "EvtDeviceD0Entry"):
        sc.check(f"driver-step:{step.split()[0]}", f"kmdftest: {step}" in logs, f"'kmdftest: {step}' in the log")
    h.kill(sc)
    h.revoke(g["node"])
    sc.finish()


def s_fx2(h, k: Kmdf, fx2_log: pathlib.Path | None) -> None:
    sc = h.scenario("kmdf-fx2", "Microsoft's kmdf_fx2 sample driver on the FX2-like device, with its test program",
                    prefix="fx2")
    if not (k.build / "osrusbfx2.sys").exists() or not (k.build / "Wdf01000.sys").exists():
        sc.check("built", False, "osrusbfx2.sys or Wdf01000.sys was not built")
        sc.finish()
        return
    g = fx2_gadget()
    h.grant(g["node"])
    h.boot(sc, "boot-init", init=True)
    k.install_runtime(sc)
    k.install_package(sc, "osrusbfx2.inf")
    h.boot(sc, "boot", DEBUG_KMDF)
    time.sleep(5)
    before = len(twd.read(fx2_log).splitlines()) if fx2_log else 0
    app = k.build / "osrusbfx2.exe"
    if app.exists():
        rc = h.wine(sc, "testapp-u", [str(app), "-u"], DEBUG_KMDF, timeout=120)
        sc.note(f"osrusbfx2.exe -u exit {rc}")
        out = twd.read(sc.dir / "testapp-u.out")
        sc.check("testapp-opens-device", "Opened the device successfully" in out,
                 " | ".join(l for l in out.splitlines() if "device" in l.lower())[:400])
        rc = h.wine(sc, "testapp-rw", [str(app), "-r", "64", "-w", "64", "-c", "10", "-v"], DEBUG_KMDF, timeout=180)
        out = twd.read(sc.dir / "testapp-rw.out")
        sc.note(f"osrusbfx2.exe -r 64 -w 64 -c 10 exit {rc}")
        wrote = len(re.findall(r"Write \(\d+\): \d+ bytes|Wrote \d+ bytes", out))
        read_ = len(re.findall(r"Read \(\d+\): \d+ bytes|Read \d+ bytes", out))
        sc.check("testapp-bulk-write-read", rc == 0 and "fail" not in out.lower() and "error" not in out.lower(),
                 f"exit {rc}; {wrote} write and {read_} read reports; last: {out.strip().splitlines()[-1][:200] if out.strip() else '-'}")
        rc = k.wine_input(sc, "testapp-p", [str(app), "-p"], b"3\n5\n6\n8\n9\n8\n7\n0\n", timeout=120)
        out = twd.read(sc.dir / "testapp-p.out")
        sc.note(f"osrusbfx2.exe -p (scripted: light all bars, bar state, switches, 7-segment, interrupt) exit {rc}")
        sc.check("testapp-bar-graph", "Bar Graph" in out or "bar" in out.lower(),
                 " | ".join(l.strip() for l in out.splitlines() if "Bar" in l or "bar" in l)[:300])
        sc.check("testapp-switches", "Switch1 is" in out,
                 " | ".join(l.strip() for l in out.splitlines() if "Switch" in l)[:300])
        sc.check("testapp-7-segment", "7 Segment mask" in out,
                 " | ".join(l.strip() for l in out.splitlines() if "Segment" in l)[:300])
    else:
        sc.check("testapp-built", False, "osrusbfx2.exe was not built")
    time.sleep(1)
    if fx2_log:
        seen = twd.read(fx2_log).splitlines()[before:]
        twd.write(sc.dir / "fx2-device.log", "\n".join(seen) + "\n")
        sc.check("device-saw-set-bargraph", any("set-bargraph 0xff" in l for l in seen),
                 f"{len(seen)} vendor/interrupt records at the device; set-bargraph: "
                 f"{[l for l in seen if 'set-bargraph' in l][:2]}")
        sc.check("device-saw-7segment", any("set-7segment" in l for l in seen),
                 f"{[l for l in seen if 'set-7segment' in l][:2]}")
    h.wine(sc, "twdinst-devices", [str(k.build / "twdinst.exe"), "devices"], twd.DEBUG_QUIET)
    logs = k.common_checks(sc, "osrusbfx2")
    for step, pattern in (("DriverEntry", r"OsrFxEvtDeviceAdd|DriverEntry"),
                          ("EvtDevicePrepareHardware", r"PrepareHardware|SelectInterfaces"),
                          ("EvtDeviceD0Entry", r"D0Entry")):
        hit = next((l.strip() for l in logs.splitlines() if re.search(pattern, l) and "osrusbfx2" in l.lower()
                    or re.search("OsrFx.*" + pattern, l)), "")
        sc.check(f"sample-step:{step}", bool(hit), hit[:300])
    h.kill(sc)
    h.revoke(g["node"])
    sc.finish()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--build", required=True, help="directory with the kmdf-binaries artifact and twdinst.exe")
    ap.add_argument("--evidence", required=True)
    ap.add_argument("--user", default="tur")
    ap.add_argument("--fx2-log", default="")
    ap.add_argument("--only", default="")
    args = ap.parse_args()
    if os.geteuid() != 0:
        print("run as root", file=sys.stderr)
        return 2
    hargs = argparse.Namespace(build=args.build, evidence=args.evidence, user=args.user, hid_log="")
    h = twd.Harness(hargs)
    h.prefixes.mkdir(parents=True, exist_ok=True)
    os.chown(h.prefixes, h.uid, h.gid)
    build = pathlib.Path(args.build).resolve()
    k = Kmdf(h, build)
    plan = [
        ("overlay", lambda: s_overlay(h, build)),
        ("kmdftest", lambda: s_kmdftest(h, k)),
        ("fx2", lambda: s_fx2(h, k, pathlib.Path(args.fx2_log) if args.fx2_log else None)),
    ]
    only = set(filter(None, args.only.split(",")))
    failed = False
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
            failed = True
    h.revoke_all()
    twd.write(h.evidence / "results.json", json.dumps({"label": "KMDF", "scenarios": h.results}, indent=2) + "\n")
    twd.write(h.evidence / "results.md", twd.markdown(h, "KMDF runtime from MIT source, Wine 11.20 + TUR patches"))
    for path in h.evidence.glob("scenarios/*/*.err"):
        if path.stat().st_size > 256 * 1024:
            with open(path, "rb") as src, gzip.open(f"{path}.gz", "wb") as dst:
                shutil.copyfileobj(src, dst)
            path.unlink()
    print(twd.read(h.evidence / "results.md"))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
