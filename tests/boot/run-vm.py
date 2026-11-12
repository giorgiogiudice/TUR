#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Boot a TUR disk image under UEFI in QEMU/KVM and observe it from outside.

    tests/boot/run-vm.py --disk disk.raw --work WORK_DIR --out OUT_DIR

What it does:

- starts QEMU with KVM acceleration only (no software-emulation fallback), OVMF
  firmware, 2 vCPUs, 4 GiB of memory, a virtio disk, user-mode networking and a
  virtio display that needs no host GPU (tests/boot/vm.py);
- captures the serial console from firmware onwards in OUT_DIR/serial.log, and
  records the kernel command line of every boot from it;
- waits, with a hard timeout, for the markers the test agent in the guest prints.
  The agent observes the first boot, sends its report over a virtio-serial port
  (WORK_DIR/report.tar), prepares the second boot and reboots; it observes the
  second boot, reports, sets GRUB's boot counter by hand and reboots; it then
  observes the third boot, the counted one, and reports again;
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
import pathlib
import re
import shutil
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import vm  # noqa: E402

MARK = vm.MARK
# Agent markers, in the order the boots produce them. FIRST-BOOT-DONE ends the
# first boot, SECOND-BOOT-DONE the second, COMPLETE the third.
MARKERS = [
    ("agent_started", re.compile(re.escape(MARK) + "START")),
    ("graphical_target", re.compile(re.escape(MARK) + r"graphical\.target reached")),
    ("graphical_session", re.compile(re.escape(MARK) + "graphical session running")),
    ("first_boot_done", re.compile(re.escape(MARK) + "FIRST-BOOT-DONE")),
    ("second_boot_done", re.compile(re.escape(MARK) + "SECOND-BOOT-DONE")),
    ("agent_complete", re.compile(re.escape(MARK) + "COMPLETE")),
]
# Console texts worth counting per boot. None is expected on a healthy boot.
PATTERNS = [
    ("kernel panic", r"Kernel panic - not syncing"),
    ("emergency mode", r"emergency mode|Emergency Mode"),
    ("grub error", r"^error: .*"),
]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--disk", required=True)
    ap.add_argument("--work", required=True, help="scratch directory (sockets, firmware variables, report.tar)")
    ap.add_argument("--out", required=True, help="artifact directory")
    ap.add_argument("--timeout", type=int, default=900, help="seconds allowed until the agent reports")
    ap.add_argument("--cpus", type=int, default=2)
    ap.add_argument("--memory", type=int, default=4096, help="MiB")
    ap.add_argument("--qemu", default="qemu-system-x86_64")
    ap.add_argument(
        "--prepare-only",
        action="store_true",
        help="stop after the second boot: the disk is then ready for the boot-health scenarios",
    )
    args = ap.parse_args()

    work, out = pathlib.Path(args.work).resolve(), pathlib.Path(args.out).resolve()
    host = out / "host"
    host.mkdir(parents=True, exist_ok=True)
    work.mkdir(parents=True, exist_ok=True)
    serial_log, report_tar = out / "serial.log", work / "report.tar"
    machine = vm.Machine(
        disk=args.disk,
        work=work,
        serial_log=serial_log,
        report_tar=report_tar,
        qemu_log=host / "qemu.log",
        cpus=args.cpus,
        memory=args.memory,
        qemu=args.qemu,
        fw_cfg={"prepare": "1"} if args.prepare_only else None,
    )
    for stale in (serial_log, report_tar, machine.variables, host / "qemu.log"):
        stale.unlink(missing_ok=True)

    command = machine.command()
    print(f"{machine.version}\nfirmware: {machine.code}\n$ {' '.join(command)}", flush=True)

    result: dict = {
        "outcome": "not-started",
        "qemu_version": machine.version,
        "firmware": machine.code,
        "command": command,
        "guest": {"cpus": args.cpus, "memory_mib": args.memory, "display": "virtio-vga (no 3D acceleration)"},
        "timeout_seconds": args.timeout,
        "milestones": {},
        "screenshots": {},
    }
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
        text = vm.clean(serial_log.read_bytes().decode("utf-8", errors="replace")) if serial_log.exists() else ""
        tail = [line for line in text.split("\n") if line.strip()][-lines:]
        print(f"----- last {len(tail)} lines of the serial console -----")
        print("\n".join(tail))
        print("----- end of serial console -----", flush=True)

    try:
        qmp = machine.start()
    except RuntimeError as error:
        print(f"QEMU did not start: {error}\n{(host / 'qemu.log').read_text()}")
        machine.quit()
        return finish("qemu-failed")
    process = machine.process

    kvm = qmp.command("query-kvm")
    result["kvm"] = kvm
    result["qemu_status"] = qmp.command("query-status")
    print(f"[{elapsed():7.1f}s] QEMU running, KVM: {kvm}", flush=True)
    if not kvm.get("enabled"):
        machine.quit()
        return finish("kvm-not-enabled")

    # ---- watch the serial console ---------------------------------------------
    watcher = vm.SerialWatcher(serial_log, elapsed, MARKERS, PATTERNS)
    result["milestones"] = watcher.milestones
    result["screenshots"] = {}
    deadline = started + args.timeout

    def shoot(name: str, attempts: int, pause: float) -> None:
        """Screenshot a boot; retry while the display still looks unrendered."""
        tries = []
        for attempt in range(1, attempts + 1):
            try:
                shot = vm.screenshot(qmp, work, out / f"screenshot-{name}.png")
            except Exception as error:
                shot = {"error": str(error), "looks_rendered": False}
            shot["at_seconds"] = elapsed()
            tries.append(shot)
            print(f"[{elapsed():7.1f}s] screenshot {name} #{attempt}: {shot}", flush=True)
            if shot["looks_rendered"] or attempt == attempts:
                break
            time.sleep(pause)
        result["screenshots"][name] = dict(tries[-1], attempts=len(tries), taken="after the agent reported")

    def shoot_after_input(name: str) -> None:
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
            shot = vm.screenshot(qmp, work, out / f"screenshot-{name}-after-input.png")
            shot["changed_fraction"] = vm.changed_fraction(
                out / f"screenshot-{name}.ppm.tmp", out / f"screenshot-{name}-after-input.ppm.tmp"
            )
            shot["at_seconds"] = elapsed()
            shot["taken"] = "after a key press and pointer movement"
            result["screenshots"][f"{name}-after-input"] = shot
            print(f"[{elapsed():7.1f}s] screenshot after input: {shot}", flush=True)
        except Exception as error:
            print(f"[{elapsed():7.1f}s] no screenshot after input: {error}", flush=True)

    outcome = "timeout"
    first_boot_shot = second_boot_shot = False
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
        if watcher.reached("second_boot_done") and not second_boot_shot:
            # The guest reboots about 45 s after this marker.
            second_boot_shot = True
            shoot("second-boot", attempts=5, pause=5)
            shoot_after_input("second-boot")
        if watcher.reached("agent_complete"):
            outcome = "agent-reported"
            break

    def summarise() -> None:
        machine.collect()
        result["boots_seen"] = watcher.boot
        result["boots"] = watcher.boots
        result["boot_sequence"] = watcher.sequence()
        result["resets_seen"] = sum(1 for event in machine.events if event["event"] == "RESET")
        result["events"] = [
            {"event": event["event"], "at_seconds": round(event["host_time"] - started, 1)}
            for event in machine.events
        ]

    summarise()
    if outcome != "agent-reported":
        missing = [name for name, _pattern in MARKERS if not watcher.reached(name)]
        print(f"[{elapsed():7.1f}s] the guest did not report: {outcome}; markers not seen: {', '.join(missing)}")
        print(f"kernel command lines seen: {json.dumps(watcher.boots, indent=1)}")
        if machine.alive():
            try:
                shot = vm.screenshot(qmp, work, out / "screenshot-at-failure.png")
                shot["taken"] = "at failure, for diagnosis"
                result["screenshots"]["at-failure"] = shot
                print(f"screenshot at failure: {shot}")
            except Exception as error:  # diagnosis only
                print(f"no screenshot at failure: {error}")
            machine.quit()
        serial_tail()
        return finish(outcome)

    # ---- screenshot of the last boot: the session has had its hold time ----------
    shoot("second-boot" if args.prepare_only else "third-boot", attempts=5, pause=5)

    # ---- power off ----------------------------------------------------------------
    result["shutdown"] = "forced"
    asked = elapsed()
    print(f"[{elapsed():7.1f}s] asking the guest to power off", flush=True)
    if machine.powerdown(timeout=180):
        result["shutdown"] = "clean"
        result["shutdown_seconds"] = round(elapsed() - asked, 1)
    else:
        print(f"[{elapsed():7.1f}s] the guest did not power off; QEMU was stopped")
    watcher.pump()
    summarise()
    print(f"[{elapsed():7.1f}s] QEMU exited ({result['shutdown']})", flush=True)
    # Keep the firmware variables: the scenarios continue on this machine.
    if machine.variables.exists():
        shutil.copyfile(machine.variables, work / "OVMF_VARS.prepared.fd")
    size = report_tar.stat().st_size if report_tar.exists() else 0
    result["report_tar_bytes"] = size
    print(f"guest report: {size} bytes; serial log: {serial_log.stat().st_size} bytes")
    print(f"boot sequence read from the console: {', '.join(watcher.sequence())}")
    return finish("agent-reported")


if __name__ == "__main__":
    sys.exit(main())
