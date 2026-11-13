#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Start a virtual machine from the TUR installer medium and use the installer
the way a person does: read the screen, type on the keyboard.

    tests/iso/install.py SCENARIO --medium tur-installer.iso --disk disk.raw \
        --work WORK_DIR --out OUT_DIR [--medium-as usb|cdrom] [--secure-boot]

The machine is the boot tests' (QEMU/KVM, OVMF, 2 vCPUs, 4 GiB, a virtio disk
with the serial number "tur-boot-test") with the medium attached and **no
network device**. The installer runs on the first console. This driver types
through QMP `send-key`, takes a screenshot of every screen through QMP
`screendump`, and reads what the installer mirrored to the serial port. It
sends nothing over the serial port: the installer the test drives is the one
a person drives.

Scenarios:

  install     Go through every screen, type the confirmation, wait for the
              installation, power off as the last screen says.
  refuse      The disk holds data. Reach the confirmation screen, type a wrong
              phrase, then the right phrase in lower case, then go back, then
              power off. The driver compares the disk's SHA-256 before and
              after: nothing may have been written.
  too-small   The disk is smaller than the installer accepts. It must say so,
              offer nothing to choose, and write nothing. Also opens the
              command line and leaves it again.
  windows-setup
              Not a test of the installer: prepares the firmware of the
              "beside Windows" proof. Opens the installer's command line and
              creates a "Windows Boot Manager" boot entry with efibootmgr,
              pointing at the Windows-like disk (--extra-disk), as Windows
              setup would have. Writes nothing to any disk.
  beside-windows
              Two disks: the Windows-like disk (--extra-disk, serial
              windows-disk-a) and a blank one. The Windows disk must be
              described as Windows; choosing it must show the Windows warning,
              and the test goes back there without typing the phrase. Then it
              installs onto the blank disk.

--extra-disk PATH=SERIAL adds a writable disk; --no-bootindex starts the
machine with no boot order from QEMU (see tests/iso/isovm.py for why);
--variables-from starts from the firmware variables an earlier run left. The
firmware's boot entries before and after the run are written to
OUT_DIR/host/efivars-before.json and efivars-after.json.

It writes OUT_DIR/result.json, OUT_DIR/serial.log, numbered screenshots, and
exits 0 if the scenario ran to its end. Whether that is a pass is decided by
tests/iso/report.py from the recorded facts.

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

DISK_SERIAL = "tur-boot-test"  # set by tests/boot/vm.py on the virtio disk
WINDOWS_SERIAL = "windows-disk-a"


class Stop(Exception):
    """The scenario cannot continue."""


class Session:
    def __init__(self, args):
        self.args = args
        self.out = pathlib.Path(args.out).resolve()
        self.work = pathlib.Path(args.work).resolve()
        self.host = self.out / "host"
        self.host.mkdir(parents=True, exist_ok=True)
        self.work.mkdir(parents=True, exist_ok=True)
        self.serial_log = self.out / "serial.log"
        self.machine = isovm.InstallMachine(
            disk=args.disk,
            work=self.work,
            serial_log=self.serial_log,
            report_tar=self.work / f"{args.scenario}-unused-report.tar",
            qemu_log=self.host / "qemu.log",
            name=f"tur-iso-{args.scenario}",
            cpus=args.cpus,
            memory=args.memory,
            qemu=args.qemu,
            medium=args.medium,
            medium_as=args.medium_as,
            secure_boot=args.secure_boot,
            extra_disks=[tuple(item.split("=", 1)) for item in args.extra_disk],
            use_bootindex=not args.no_bootindex,
            variables_from=args.variables_from,
        )
        for stale in (self.serial_log, self.machine.variables, self.host / "qemu.log"):
            stale.unlink(missing_ok=True)
        for stale in self.out.glob("*.png"):
            stale.unlink()
        self.transcript = isovm.Transcript(self.serial_log)
        self.cursor = 0
        self.shots = 0
        self.started = time.monotonic()
        self.result: dict = {
            "scenario": args.scenario,
            "outcome": "not-started",
            "medium": str(args.medium),
            "medium_attached_as": args.medium_as,
            "secure_boot_firmware": bool(args.secure_boot),
            "network_devices": 0,
            "boot_order_from_qemu": not args.no_bootindex,
            "extra_disks": list(args.extra_disk),
            "variables_from": args.variables_from,
            "disk": str(args.disk),
            "disk_serial": DISK_SERIAL,
            "guest": {"cpus": args.cpus, "memory_mib": args.memory},
            "qemu_version": self.machine.version,
            "firmware": self.machine.code,
            "command": self.machine.command(),
            "steps": [],
            "screenshots": [],
            "facts": {},
        }
        self.qmp = None
        self.keyboard = None

    def elapsed(self) -> float:
        return round(time.monotonic() - self.started, 1)

    def say(self, text: str) -> None:
        print(f"[{self.elapsed():7.1f}s] {text}", flush=True)

    def step(self, text: str) -> None:
        self.result["steps"].append({"at_seconds": self.elapsed(), "what": text})
        self.say(text)

    def start(self) -> None:
        self.say(f"{self.machine.version}; firmware {self.machine.code}")
        self.say("$ " + " ".join(self.machine.command()))
        self.qmp = self.machine.start()
        self.keyboard = isovm.Keyboard(self.qmp)
        kvm = self.qmp.command("query-kvm")
        self.result["kvm"] = kvm
        if not kvm.get("enabled"):
            raise Stop("KVM is not enabled")
        self.result["outcome"] = "started"
        self.choose_text_mode()

    def choose_text_mode(self) -> None:
        """Start the boot menu's text-mode entry, as a person would: by its key.

        The medium starts the live session by default. Its menu gives the
        text installer the hotkey "t" (installer/iso/third-party-licensed/grub).
        The firmware says on the serial port when it starts the medium; from
        then on "t" is pressed a few times a second until the menu has surely
        come and gone. A key pressed before the menu is drawn waits for it;
        one pressed after the kernel started is flushed when the installer
        takes the console."""
        try:
            self.wait(r'BdsDxe: starting Boot\w+ "[^"]*(TURINSTALLERMEDIUM|DVD-ROM)', 60, "the firmware to start the medium")
            self.result["facts"]["firmware_started_medium_seconds"] = self.elapsed()
        except Stop:
            self.say("the firmware did not say when it started the medium; pressing the key anyway")
        self.cursor = 0
        presses = 0
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            self.keyboard.press("t")
            presses += 1
            time.sleep(0.25)
        self.result["facts"]["boot_menu"] = {"entry": "text mode", "hotkey": "t", "presses": presses}
        self.step(f"pressed the boot menu's text-mode key {presses} times")

    def wait(self, pattern: str, timeout: float, what: str) -> "re.Match":
        """Wait for text on the serial port after the cursor; move the cursor past it."""
        deadline = time.monotonic() + timeout
        while True:
            self.transcript.pump()
            match = self.transcript.search(pattern, self.cursor)
            if match:
                self.cursor += match.end()
                return match
            if not self.machine.alive():
                raise Stop(f"QEMU exited while waiting for {what}")
            if time.monotonic() > deadline:
                raise Stop(f"timeout after {timeout:.0f} s waiting for {what}")
            time.sleep(0.5)
            self.qmp.drain()

    def wait_prompt(self, screen: str, timeout: float = 120) -> None:
        self.wait(rf"TUR-INSTALLER: (prompt|waiting) screen={screen}\b", timeout, f"the {screen} screen")
        # Let the console finish drawing before a screenshot or a key press.
        time.sleep(1.0)
        self.transcript.pump()

    def shot(self, name: str) -> None:
        self.shots += 1
        target = self.out / f"{self.shots:02d}-{name}.png"
        try:
            facts = vm.screenshot(self.qmp, self.work, target)
        except Exception as error:  # evidence only
            facts = {"error": str(error)}
        target.with_suffix(".ppm.tmp").unlink(missing_ok=True)
        facts.update({"file": target.name, "at_seconds": self.elapsed(), "screen": name})
        self.result["screenshots"].append(facts)
        self.say(f"screenshot {target.name}: {facts}")

    def type_line(self, text: str, shown: str | None = None) -> None:
        self.step(f"typed: {shown if shown is not None else repr(text)} and Enter")
        self.keyboard.line(text)

    def states_since(self, position: int) -> list[dict]:
        self.transcript.pump()
        return [dict(isovm.fields(state), _line=state) for state in self.transcript.states(position)]

    def wait_power_off(self, timeout: float = 240) -> bool:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if not self.machine.alive():
                self.machine.collect()
                return True
            time.sleep(0.5)
        return False

    def firmware_entries(self) -> None:
        """The firmware's boot entries before and after this run, read from the files."""
        for label, path in (("before", self.machine.variables_template), ("after", self.machine.variables)):
            try:
                entries = efivars.boot_entries(pathlib.Path(path))
            except (OSError, ValueError) as error:
                entries = {"error": str(error), "file": str(path)}
            (self.host / f"efivars-{label}.json").write_text(json.dumps(entries, indent=2) + "\n")
            self.result.setdefault("firmware_boot_entries", {})[label] = entries

    def finish(self, outcome: str) -> int:
        self.transcript.pump()
        self.machine.collect()
        if not self.machine.alive():
            self.firmware_entries()
        self.result["outcome"] = outcome
        self.result["elapsed_seconds"] = self.elapsed()
        self.result["events"] = [
            {"event": event["event"], "at_seconds": round(event["host_time"] - self.started, 1)}
            for event in self.machine.events
        ]
        self.result["installer_states"] = self.transcript.states()
        (self.out / "installer-screens.txt").write_text("\n".join(self.transcript.screen_lines()) + "\n")
        lsblk = self.transcript.tagged("TUR-INSTALLER-LSBLK")
        if lsblk:
            (self.out / "live-lsblk.json").write_text("\n".join(lsblk) + "\n")
        links = self.transcript.tagged("TUR-INSTALLER-LINKS")
        if links:
            (self.out / "live-disk-links.txt").write_text("\n".join(links) + "\n")
        (self.out / "result.json").write_text(json.dumps(self.result, indent=2) + "\n")
        self.say(f"outcome: {outcome}")
        return 0 if outcome == "completed" else 1

    def abort(self, reason: str) -> int:
        self.say(f"STOPPED: {reason}")
        self.result["stopped_because"] = reason
        if self.machine.alive() and self.qmp is not None:
            try:
                self.shot("at-failure")
            except Exception as error:  # diagnosis only
                self.say(f"no screenshot at failure: {error}")
        self.machine.quit()
        self.transcript.pump()
        tail = [line for line in self.transcript.text.split("\n") if line.strip()][-120:]
        print(f"----- last {len(tail)} lines of the serial console -----")
        print("\n".join(tail))
        print("----- end of serial console -----", flush=True)
        return self.finish("stopped")

    # ---- the screens ---------------------------------------------------------------

    def to_welcome(self) -> None:
        facts = self.result["facts"]
        self.wait(r"TUR-INSTALLER: start ", 420, "the installer to start")
        facts["installer_started_seconds"] = self.elapsed()
        self.step("the installer started")
        self.wait_prompt("welcome", 600)
        facts["welcome_seconds"] = self.elapsed()
        for state in self.states_since(0):
            line = state["_line"]
            if line.startswith("start "):
                facts["start"] = {k: v for k, v in state.items() if k != "_line"}
            elif line.startswith("cmdline "):
                facts["kernel_command_line"] = line[len("cmdline "):]
            elif line.startswith("live-medium "):
                facts["live_medium"] = line[len("live-medium "):]
            elif line.startswith("check "):
                facts.setdefault("checks", {}).update({k: v for k, v in state.items() if k != "_line"})
            elif line.startswith("blocked "):
                facts["blocked"] = line
        self.step(f"welcome screen; checks: {facts.get('checks')}")
        self.shot("welcome")

    def to_disks(self) -> dict | None:
        """Press Enter on the welcome screen; return the test disk as listed."""
        before = self.cursor
        self.type_line("", "nothing")
        self.wait_prompt("disks")
        return self.read_disks(before)

    def read_disks(self, since: int) -> dict | None:
        disks = [state for state in self.states_since(since) if state["_line"].startswith("disk ")]
        latest: dict[str, dict] = {}
        for disk in disks:
            latest[disk.get("name", "?")] = {k: v for k, v in disk.items() if k != "_line"}
        self.result["facts"]["disks"] = list(latest.values())
        totals = [state for state in self.states_since(since) if state["_line"].startswith("disks ")]
        if totals:
            self.result["facts"]["disks_selectable"] = totals[-1].get("selectable")
        target = next((disk for disk in latest.values() if disk.get("serial") == DISK_SERIAL), None)
        self.result["facts"]["target_as_listed"] = target
        self.step(f"disk screen; disks listed: {list(latest.values())}")
        return target

    def to_confirm(self, target: dict, serial: str = DISK_SERIAL) -> str:
        before = self.cursor
        self.type_line(target["index"])
        match = self.wait(r'TUR-INSTALLER: confirm name=\S+ by_id=(\S+) phrase="([^"]*)"', 60, "the confirmation screen")
        shown = match.group(2)
        self.wait_prompt("confirm")
        expected = isovm.confirmation_phrase(serial)
        warning = self.transcript.search(r"TUR-INSTALLER: confirm-warning (.*)$", before)
        self.result["facts"].setdefault("warnings", {})[serial] = warning.group(1).strip() if warning else None
        facts = self.result["facts"]
        facts["confirmation"] = {
            "by_id": match.group(1),
            "phrase_shown": shown,
            "phrase_computed_by_the_test": expected,
            "phrase_matches": shown == expected,
        }
        self.step(f"confirmation screen for {match.group(1)}; phrase shown: {shown!r}")
        return expected


def scenario_install(s: Session) -> None:
    facts = s.result["facts"]
    s.to_welcome()
    target = s.to_disks()
    s.shot("disks")
    if not target or target.get("status") != "ok" or not target.get("index", "").isdigit():
        raise Stop(f"the test disk is not offered: {target}")
    phrase = s.to_confirm(target)
    s.shot("confirm")
    s.type_line(phrase)
    s.wait(r"TUR-INSTALLER: confirm result=accepted", 60, "the confirmation to be accepted")
    s.wait(r"TUR-INSTALLER: revalidated ", 60, "the disk to be looked at again")
    match = s.wait(r'TUR-INSTALLER: installing device=(\S+) command="([^"]*)"', 60, "the installation to start")
    facts["install_device"], facts["install_command"] = match.group(1), match.group(2)
    install_started = s.elapsed()
    s.step(f"installation started: {match.group(2)}")
    time.sleep(25)
    s.shot("installing")
    match = s.wait(r"TUR-INSTALLER: install-finished status=(\d+) seconds=(\d+)", 2400, "the installation to finish")
    facts["install_status"], facts["install_seconds"] = int(match.group(1)), int(match.group(2))
    facts["install_seconds_seen_by_host"] = round(s.elapsed() - install_started, 1)
    match = s.wait(r"TUR-INSTALLER: result=(success|failed)\b[^\n]*", 120, "the result")
    facts["result"] = match.group(1)
    facts["result_line"] = match.group(0)
    s.step(f"installer result: {match.group(0)}")
    s.wait_prompt("finished" if facts["result"] == "success" else "failed", 60)
    s.shot("done" if facts["result"] == "success" else "failed")
    if facts["result"] != "success":
        raise Stop("the installer reported a failure")
    s.type_line("", "nothing")
    facts["powered_off_by_installer"] = s.wait_power_off()
    s.step(f"the machine powered off by itself: {facts['powered_off_by_installer']}")
    if not facts["powered_off_by_installer"]:
        s.machine.quit()


def scenario_refuse(s: Session) -> None:
    facts = s.result["facts"]
    facts["disk_sha256_before"] = isovm.sha256(s.args.disk)
    s.step(f"disk SHA-256 before: {facts['disk_sha256_before']}")
    s.to_welcome()
    target = s.to_disks()
    s.shot("disks-with-data")
    if not target or target.get("status") != "ok":
        raise Stop(f"the test disk is not offered: {target}")
    phrase = s.to_confirm(target)
    s.shot("confirm-existing-data")
    attempts = []
    for label, text in (("a wrong phrase", "ERASE WRONG123"), ("the right phrase in lower case", phrase.lower())):
        s.type_line(text)
        match = s.wait(r"TUR-INSTALLER: confirm result=(\w+)(?: code=(\S+))?", 60, "the answer to " + label)
        attempts.append({"typed": text, "what": label, "result": match.group(1), "code": match.group(2)})
        s.step(f"{label}: {match.group(1)} {match.group(2) or ''}")
        if match.group(1) != "mismatch":
            facts["refusals"] = attempts
            raise Stop(f"{label} was not refused")
        time.sleep(1.5)
        if len(attempts) == 1:
            s.shot("wrong-phrase-refused")
        s.wait_prompt("confirm")
    facts["refusals"] = attempts
    s.type_line("", "nothing")
    match = s.wait(r"TUR-INSTALLER: confirm result=(\w+)", 60, "going back")
    facts["enter_alone"] = match.group(1)
    s.wait_prompt("disks")
    s.shot("back-at-disks")
    s.type_line("off")
    s.wait(r"TUR-INSTALLER: power-off", 60, "the power-off")
    facts["powered_off_by_installer"] = s.wait_power_off()
    if not facts["powered_off_by_installer"]:
        s.machine.quit()
    facts["installing_seen"] = bool(s.transcript.search(r"TUR-INSTALLER: installing "))
    facts["disk_sha256_after"] = isovm.sha256(s.args.disk)
    facts["disk_unchanged"] = facts["disk_sha256_before"] == facts["disk_sha256_after"]
    s.step(f"disk SHA-256 after: {facts['disk_sha256_after']}; unchanged: {facts['disk_unchanged']}")


def scenario_too_small(s: Session) -> None:
    facts = s.result["facts"]
    facts["disk_sha256_before"] = isovm.sha256(s.args.disk)
    s.to_welcome()
    before = s.cursor
    s.type_line("", "nothing")
    s.wait(r"TUR-INSTALLER: no-target code=(\S+)", 60, "the installer to say there is no disk")
    s.wait_prompt("disks")
    target = s.read_disks(before)
    facts["no_target_code"] = "TUR-INSTALL-MEDIUM-003" if s.transcript.search(r"no-target code=TUR-INSTALL-MEDIUM-003") else None
    s.shot("disk-too-small")
    if not target or target.get("status") != "too-small":
        raise Stop(f"the small disk is not reported as too small: {target}")
    s.type_line("1")
    match = s.wait(r"TUR-INSTALLER: choice result=(\w+)", 60, "the answer to choosing a disk anyway")
    facts["choosing_anyway"] = match.group(1)
    s.wait_prompt("disks")
    # The command line: open it, run one command that writes to the serial
    # port, leave it.
    s.type_line("shell")
    s.wait(r"TUR-INSTALLER: shell=started", 60, "the command line")
    time.sleep(2)
    s.type_line("echo shell-ok-$((6*7)) > /dev/ttyS0")
    s.wait(r"shell-ok-42", 60, "the command's output")
    facts["shell_command_ran"] = True
    s.type_line("lsblk")
    time.sleep(2)
    s.shot("command-line")
    s.type_line("exit")
    s.wait(r"TUR-INSTALLER: shell=ended", 60, "leaving the command line")
    facts["shell_left"] = True
    s.wait_prompt("disks")
    s.type_line("off")
    s.wait(r"TUR-INSTALLER: power-off", 60, "the power-off")
    facts["powered_off_by_installer"] = s.wait_power_off()
    if not facts["powered_off_by_installer"]:
        s.machine.quit()
    facts["installing_seen"] = bool(s.transcript.search(r"TUR-INSTALLER: installing "))
    facts["disk_sha256_after"] = isovm.sha256(s.args.disk)
    facts["disk_unchanged"] = facts["disk_sha256_before"] == facts["disk_sha256_after"]
    s.step(f"disk unchanged: {facts['disk_unchanged']}")


def scenario_windows_setup(s: Session) -> None:
    facts = s.result["facts"]
    s.to_welcome()
    s.type_line("shell")
    s.wait(r"TUR-INSTALLER: shell=started", 60, "the command line")
    time.sleep(2)
    command = (
        f"efibootmgr --create --disk /dev/disk/by-id/virtio-{WINDOWS_SERIAL} --part 1 "
        "--loader '\\EFI\\Microsoft\\Boot\\bootmgfw.efi' --label 'Windows Boot Manager' "
        "> /dev/ttyS0 2>&1; echo efibootmgr-create-status-$? > /dev/ttyS0"
    )
    s.type_line(command)
    match = s.wait(r"efibootmgr-create-status-(\d+)", 60, "efibootmgr")
    facts["efibootmgr_status"] = int(match.group(1))
    s.type_line("efibootmgr -v > /dev/ttyS0 2>&1")
    time.sleep(3)
    s.shot("windows-entry-created")
    s.type_line("exit")
    s.wait(r"TUR-INSTALLER: shell=ended", 60, "leaving the command line")
    s.wait_prompt("welcome")
    s.type_line("off")
    s.wait(r"TUR-INSTALLER: power-off", 60, "the power-off")
    facts["powered_off_by_installer"] = s.wait_power_off()
    if not facts["powered_off_by_installer"]:
        s.machine.quit()
    if facts["efibootmgr_status"] != 0:
        raise Stop(f"efibootmgr failed with status {facts['efibootmgr_status']}")


def scenario_beside_windows(s: Session) -> None:
    facts = s.result["facts"]
    s.to_welcome()
    target = s.to_disks()
    disks = {disk.get("serial"): disk for disk in facts.get("disks", [])}
    windows = disks.get(WINDOWS_SERIAL)
    facts["windows_disk_as_listed"] = windows
    s.shot("disks-beside-windows")
    if not windows or not target or target.get("status") != "ok":
        raise Stop(f"the two disks are not listed as expected: {disks}")
    # Choose the Windows disk: the warning must be there. Go back without the phrase.
    if windows.get("index", "-").isdigit():
        s.to_confirm(windows, WINDOWS_SERIAL)
        s.shot("confirm-windows-disk")
        s.type_line("", "nothing")
        match = s.wait(r"TUR-INSTALLER: confirm result=(\w+)", 60, "going back from the Windows disk")
        facts["windows_disk_enter_alone"] = match.group(1)
        s.wait_prompt("disks")
    else:
        facts["windows_disk_enter_alone"] = "not offered"
    phrase = s.to_confirm(target)
    s.shot("confirm-second-disk")
    s.type_line(phrase)
    s.wait(r"TUR-INSTALLER: confirm result=accepted", 60, "the confirmation to be accepted")
    match = s.wait(r'TUR-INSTALLER: installing device=(\S+) command="([^"]*)"', 90, "the installation to start")
    facts["install_device"], facts["install_command"] = match.group(1), match.group(2)
    match = s.wait(r"TUR-INSTALLER: install-finished status=(\d+) seconds=(\d+)", 2400, "the installation to finish")
    facts["install_status"], facts["install_seconds"] = int(match.group(1)), int(match.group(2))
    match = s.wait(r"TUR-INSTALLER: result=(success|failed)\b[^\n]*", 120, "the result")
    facts["result"] = match.group(1)
    s.wait_prompt("finished" if facts["result"] == "success" else "failed", 60)
    s.shot("finished-beside-windows")
    if facts["result"] != "success":
        raise Stop("the installer reported a failure")
    s.type_line("", "nothing")
    facts["powered_off_by_installer"] = s.wait_power_off()
    if not facts["powered_off_by_installer"]:
        s.machine.quit()


SCENARIOS = {
    "install": scenario_install,
    "refuse": scenario_refuse,
    "too-small": scenario_too_small,
    "windows-setup": scenario_windows_setup,
    "beside-windows": scenario_beside_windows,
}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("scenario", choices=sorted(SCENARIOS))
    ap.add_argument("--medium", required=True)
    ap.add_argument("--disk", required=True)
    ap.add_argument("--work", required=True, help="scratch directory (sockets, firmware variables)")
    ap.add_argument("--out", required=True, help="artifact directory of this scenario")
    ap.add_argument("--medium-as", choices=["usb", "cdrom"], default="usb")
    ap.add_argument("--secure-boot", action="store_true", help="OVMF with Secure Boot enforced and Microsoft's keys")
    ap.add_argument("--extra-disk", action="append", default=[], metavar="PATH=SERIAL", help="a further writable disk")
    ap.add_argument("--no-bootindex", action="store_true", help="pass QEMU no boot order (OVMF then keeps BootOrder)")
    ap.add_argument("--variables-from", help="start from these firmware variables")
    ap.add_argument("--cpus", type=int, default=2)
    ap.add_argument("--memory", type=int, default=4096, help="MiB")
    ap.add_argument("--qemu", default="qemu-system-x86_64")
    args = ap.parse_args()

    session = Session(args)
    try:
        session.start()
        SCENARIOS[args.scenario](session)
    except (Stop, RuntimeError, ConnectionError) as error:
        return session.abort(str(error))
    # Keep the firmware variables: the installed disk is started with them next.
    if session.machine.variables.exists():
        (session.out / "host" / "OVMF_VARS.after.fd").write_bytes(session.machine.variables.read_bytes())
    return session.finish("completed")


if __name__ == "__main__":
    sys.exit(main())
