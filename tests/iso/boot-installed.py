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

With --extra-disk, --no-bootindex and --variables-from (as for
tests/iso/install.py) it starts the disk beside another disk, from the
firmware variables an earlier run left, without a boot order from QEMU. It
writes the firmware's boot entries before and after to
OUT_DIR/host/efivars-before.json and efivars-after.json.

With --windows-entry (ADR-0028, a disk with TUR installed beside Windows) it
instead chooses "Windows" in TUR's boot menu, by its key "w", pressed a few
times a second once the menu is on the screen (or after a few seconds if the
serial port does not show it), as a person would. The test disk holds a
stand-in for Windows Boot Manager (tests/iso/standin, not a Microsoft
program) that writes "TUR-WINDOWS-STANDIN: started file=... device=..." and
powers the machine off. The outcome is then "windows-started" with the file
and device path it reports, or "tur-started" when a rendered screen came
instead. Exit status 0 for either: tests/iso/beside-report.py judges.

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
import efivars  # noqa: E402
import isovm  # noqa: E402
from isovm import vm  # noqa: E402


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--disk", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--secure-boot", action="store_true")
    ap.add_argument("--extra-disk", action="append", default=[], metavar="PATH=SERIAL")
    ap.add_argument("--no-bootindex", action="store_true")
    ap.add_argument("--variables-from")
    ap.add_argument("--timeout", type=int, default=600, help="seconds allowed until a rendered screen")
    ap.add_argument("--windows-entry", action="store_true", help="choose Windows in TUR's boot menu (key w)")
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
        extra_disks=[tuple(item.split("=", 1)) for item in args.extra_disk],
        use_bootindex=not args.no_bootindex,
        variables_from=args.variables_from,
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
        "boot_order_from_qemu": not args.no_bootindex,
        "extra_disks": list(args.extra_disk),
        "variables_from": args.variables_from,
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
        for label, path in (("before", machine.variables_template), ("after", machine.variables)):
            try:
                entries = efivars.boot_entries(pathlib.Path(path))
            except (OSError, ValueError) as error:
                entries = {"error": str(error), "file": str(path)}
            (host / f"efivars-{label}.json").write_text(json.dumps(entries, indent=2) + "\n")
            result.setdefault("firmware_boot_entries", {})[label] = entries
        result["outcome"] = outcome
        result["elapsed_seconds"] = elapsed()
        result["events"] = [
            {"event": event["event"], "at_seconds": round(event["host_time"] - started, 1)} for event in machine.events
        ]
        (out / "first-start.json").write_text(json.dumps(result, indent=2) + "\n")
        print(f"[{elapsed():7.1f}s] outcome: {outcome}", flush=True)
        return 0 if outcome in ("rendered", "windows-started", "tur-started") else 1

    print(f"{machine.version}\n$ {' '.join(machine.command())}", flush=True)
    try:
        qmp = machine.start()
    except RuntimeError as error:
        print(f"QEMU did not start: {error}")
        machine.quit()
        return finish("qemu-failed")
    result["kvm"] = qmp.command("query-kvm")

    if args.windows_entry:
        return windows_entry(args, machine, qmp, result, out, work, serial_log, elapsed, finish)

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


def windows_entry(args, machine, qmp, result, out, work, serial_log, elapsed, finish) -> int:
    """Choose "Windows" in TUR's boot menu and see what starts."""
    import re

    transcript = isovm.Transcript(serial_log)
    menu_seen = None
    shot_taken = False
    pressed = 0
    started = time.monotonic()
    deadline = started + args.timeout
    standin = None
    result["windows_entry"] = {"key": "w"}
    while time.monotonic() < deadline and machine.alive():
        transcript.pump()
        if menu_seen is None and re.search(r"\bWindows\b", transcript.text) and not re.search(r"TUR-WINDOWS-STANDIN",
                                                                                         transcript.text):
            menu_seen = elapsed()
        standin = re.search(r"TUR-WINDOWS-STANDIN: started file=(\S+)", transcript.text)
        if standin:
            break
        # The key from four seconds after power-on (a key pressed before
        # GRUB's menu waits for it), or as soon as the menu is seen on the
        # serial port; a picture of the screen just before the first press.
        if menu_seen is not None or time.monotonic() - started > 4:
            if not shot_taken:
                try:
                    result["screenshots"]["boot-menu"] = vm.screenshot(qmp, work, out / "boot-menu.png")
                except Exception as error:  # evidence only
                    result["screenshots"]["boot-menu"] = {"error": str(error)}
                shot_taken = True
            try:
                qmp.command("send-key", keys=[{"type": "qcode", "data": "w"}], **{"hold-time": 60})
                pressed += 1
            except Exception:
                pass
        time.sleep(0.4)
    # The stand-in may have written its line and powered off between two looks.
    time.sleep(1)
    transcript.pump()
    standin = re.search(r"TUR-WINDOWS-STANDIN: started file=(\S+)", transcript.text)
    entry = result["windows_entry"]
    entry.update({"menu_seen_on_serial_seconds": menu_seen, "presses": pressed})
    if standin:
        entry["started_seconds"] = elapsed()
        # The stand-in prints its device path, then powers off three seconds later.
        end = time.monotonic() + 20
        device = None
        while time.monotonic() < end:
            transcript.pump()
            device = re.search(r"TUR-WINDOWS-STANDIN: device=(\S+)", transcript.text)
            if device or not machine.alive():
                break
            time.sleep(0.3)
        if machine.alive():
            try:
                result["screenshots"]["windows-standin"] = vm.screenshot(qmp, work, out / "windows-standin.png")
            except Exception as error:  # evidence only
                result["screenshots"]["windows-standin"] = {"error": str(error)}
        transcript.pump()
        device = re.search(r"TUR-WINDOWS-STANDIN: device=(\S+)", transcript.text)
        entry["file"] = standin.group(1)
        entry["device"] = device.group(1) if device else None
        partition = re.search(r"TUR-WINDOWS-STANDIN: partition=(\d+) guid=([0-9A-Fa-f-]{36})", transcript.text)
        partuuid = re.search(r"HD\(\d+,GPT,([0-9A-Fa-f-]{36})", entry["device"] or "")
        entry["partition_number"] = int(partition.group(1)) if partition else None
        entry["partition_uuid"] = (partition.group(2) if partition else partuuid.group(1) if partuuid else "").upper() or None
        try:
            machine.process.wait(timeout=30)
            entry["powered_off_by_itself"] = True
        except Exception:
            entry["powered_off_by_itself"] = False
        machine.quit()
        return finish("windows-started")
    if machine.alive():
        try:
            result["screenshots"]["at-end"] = vm.screenshot(qmp, work, out / "windows-entry-at-end.png")
            result["screenshots"]["at-end"]["looks_rendered"] = result["screenshots"]["at-end"].get("looks_rendered")
        except Exception as error:  # diagnosis only
            result["screenshots"]["at-end"] = {"error": str(error)}
    rendered = bool(result["screenshots"].get("at-end", {}).get("looks_rendered"))
    machine.quit()
    return finish("tur-started" if rendered else "nothing-started")


if __name__ == "__main__":
    sys.exit(main())
