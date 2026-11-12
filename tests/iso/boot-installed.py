#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Start a disk the installer medium installed, without the medium, and watch
its screen.

    tests/iso/boot-installed.py --disk disk.raw --work WORK_DIR --out OUT_DIR
        [--secure-boot]

For a disk that holds the product image: it carries no test agent and writes
nothing to the serial port, so the only things visible from outside are the
virtual display while it runs and the disk afterwards. This driver

- starts the disk in the boot tests' machine with no installer medium and no
  network device (QEMU/KVM, OVMF, fresh firmware variables: the first start
  goes through the removable-media path, as on a computer that has never seen
  this disk);
- takes a screenshot every few seconds until the display shows a rendered
  graphical screen, twice in a row some seconds apart;
- lets the system run on, so that its boot health gate can finish and the
  journal can reach the disk;
- presses a key and moves the pointer, and takes another screenshot;
- asks for power-off through ACPI and waits; stops QEMU if the guest does not.

It writes OUT_DIR/first-start.json and the screenshots, and exits 0 if a
rendered screen was seen. tests/boot/inspect-disk.py then reads the journal
from the disk, and tests/iso/report.py decides what it all shows.

Disks that hold the boot-test image are started by tests/boot/run-vm.py
instead, which runs the boot test's checks.

Run in CI only (ADR-0002): never on a shared or production machine.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import isovm  # noqa: E402
from isovm import vm  # noqa: E402


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--disk", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--secure-boot", action="store_true")
    ap.add_argument("--timeout", type=int, default=600, help="seconds allowed until a rendered screen")
    ap.add_argument("--run-on", type=int, default=75, help="seconds to keep running after the rendered screen")
    ap.add_argument("--cpus", type=int, default=2)
    ap.add_argument("--memory", type=int, default=4096, help="MiB")
    ap.add_argument("--qemu", default="qemu-system-x86_64")
    args = ap.parse_args()

    out, work = pathlib.Path(args.out).resolve(), pathlib.Path(args.work).resolve()
    host = out / "host"
    host.mkdir(parents=True, exist_ok=True)
    work.mkdir(parents=True, exist_ok=True)
    serial_log = out / "serial.log"
    machine = isovm.InstallMachine(
        disk=args.disk,
        work=work,
        serial_log=serial_log,
        report_tar=work / "first-start-unused-report.tar",
        qemu_log=host / "qemu.log",
        name="tur-iso-first-start",
        cpus=args.cpus,
        memory=args.memory,
        qemu=args.qemu,
        medium=None,
        secure_boot=args.secure_boot,
    )
    for stale in (serial_log, machine.variables, host / "qemu.log"):
        stale.unlink(missing_ok=True)
    started = time.monotonic()

    def elapsed() -> float:
        return round(time.monotonic() - started, 1)

    result: dict = {
        "outcome": "not-started",
        "medium_attached": False,
        "network_devices": 0,
        "secure_boot_firmware": bool(args.secure_boot),
        "firmware": machine.code,
        "qemu_version": machine.version,
        "command": machine.command(),
        "samples": [],
        "screenshots": {},
    }

    def finish(outcome: str) -> int:
        for leftover in out.glob("*.ppm.tmp"):
            leftover.unlink()
        machine.collect()
        result["outcome"] = outcome
        result["elapsed_seconds"] = elapsed()
        result["events"] = [
            {"event": event["event"], "at_seconds": round(event["host_time"] - started, 1)} for event in machine.events
        ]
        (out / "first-start.json").write_text(json.dumps(result, indent=2) + "\n")
        print(f"[{elapsed():7.1f}s] outcome: {outcome}", flush=True)
        return 0 if outcome == "rendered" else 1

    print(f"{machine.version}\n$ {' '.join(machine.command())}", flush=True)
    try:
        qmp = machine.start()
    except RuntimeError as error:
        print(f"QEMU did not start: {error}")
        machine.quit()
        return finish("qemu-failed")
    result["kvm"] = qmp.command("query-kvm")

    rendered_at = None
    confirmed = False
    deadline = started + args.timeout
    while time.monotonic() < deadline and machine.alive():
        time.sleep(6)
        try:
            shot = vm.screenshot(qmp, work, out / "first-start.png")
        except Exception as error:
            shot = {"error": str(error), "looks_rendered": False}
        shot["at_seconds"] = elapsed()
        result["samples"].append(shot)
        print(f"[{elapsed():7.1f}s] display: {shot}", flush=True)
        if shot.get("looks_rendered"):
            if rendered_at is None:
                rendered_at = elapsed()
            elif elapsed() - rendered_at >= 12:
                confirmed = True
                break
        else:
            rendered_at = None
    if not confirmed:
        if machine.alive():
            try:
                result["screenshots"]["at-failure"] = vm.screenshot(qmp, work, out / "first-start-at-failure.png")
            except Exception as error:  # diagnosis only
                print(f"no screenshot at failure: {error}")
        machine.quit()
        return finish("no-rendered-screen" if machine.starts else "qemu-failed")

    result["rendered_first_seen_seconds"] = rendered_at
    print(f"[{elapsed():7.1f}s] a rendered screen has been up since {rendered_at} s; running on for {args.run_on} s", flush=True)
    time.sleep(args.run_on)
    try:
        shot = vm.screenshot(qmp, work, out / "first-start.png")
        shot["at_seconds"] = elapsed()
        result["screenshots"]["first-start"] = shot
        qmp.command("send-key", keys=[{"type": "qcode", "data": "shift"}])
        qmp.command(
            "input-send-event",
            events=[
                {"type": "rel", "data": {"axis": "x", "value": 40}},
                {"type": "rel", "data": {"axis": "y", "value": 40}},
            ],
        )
        time.sleep(4)
        after = vm.screenshot(qmp, work, out / "first-start-after-input.png")
        after["changed_fraction"] = vm.changed_fraction(
            out / "first-start.ppm.tmp", out / "first-start-after-input.ppm.tmp"
        )
        after["at_seconds"] = elapsed()
        result["screenshots"]["first-start-after-input"] = after
        print(f"[{elapsed():7.1f}s] after input: {after}", flush=True)
    except Exception as error:  # evidence only
        print(f"[{elapsed():7.1f}s] screenshots after the run-on failed: {error}")
    result["still_running_after_run_on"] = machine.alive()

    asked = elapsed()
    result["shutdown"] = "forced"
    if machine.powerdown(timeout=150):
        result["shutdown"] = "clean"
        result["shutdown_seconds"] = round(elapsed() - asked, 1)
    print(f"[{elapsed():7.1f}s] power-off: {result['shutdown']}", flush=True)
    return finish("rendered")


if __name__ == "__main__":
    sys.exit(main())
