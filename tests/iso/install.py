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
  same-disk   ADR-0029: one disk laid out as Windows leaves it after "Shrink
              Volume" (tests/iso/make-windows-shared-disk.sh) is the main
              disk. It must be listed with room beside Windows; choosing it
              asks how; "1" (beside Windows) leads to the confirmation, where
              the erase phrase is refused and the "BESIDE" phrase installs
              TUR into the free space. Powers off as the last screen says.
  same-disk-cut
              The same, but the power is cut (QEMU stopped at once, with no
              warning to the guest) when the installation reaches --cut-at
              (and --cut-delay seconds more). What happened before the cut is
              in result.json; tests/iso/windows-disk.py then compares the
              disk with what it held before.
              With --dualboot-after, at the last screen it opens the command
              line and runs TUR's command for two systems (ADR-0029, in the
              image, which the medium's environment is): "default windows",
              "default tur" (each followed by "status"), then
              "restart-windows", which sets the firmware's BootNext and
              restarts; the run then waits for the stand-in for Windows Boot
              Manager to start, which powers the machine off.
  shrink      Issue 64, step 2: the main disk is laid out as Windows with no
              free space and a large, nearly empty Windows partition. It must be
              offered for shrinking; "3" (shrink), --tur-gib GiB, the default
              system, then the "SHRINK" phrase (the erase and beside phrases are
              refused), the installation.
  beside-refused
              Disks laid out as Windows on which installing beside Windows is
              not possible (no free space; a full EFI system partition). Each
              must be listed with the reason and code, choosing one must go
              straight to the erase confirmation, which the test leaves with
              Enter. Every disk's SHA-256 is compared before and after.

--windows-serial and --windows-partition name the disk and partition of the
"Windows Boot Manager" entry windows-setup creates (default: windows-disk-a, 1).

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
        f"efibootmgr --create --disk /dev/disk/by-id/virtio-{s.args.windows_serial} --part {s.args.windows_partition} "
        "--loader '\\EFI\\Microsoft\\Boot\\bootmgfw.efi' --label 'Windows Boot Manager' "
        "> /dev/ttyS0 2>&1; echo efibootmgr-create-status-$? > /dev/ttyS0"
    )
    s.type_line(command)
    match = s.wait(r"efibootmgr-create-status-(\d+)", 60, "efibootmgr")
    facts["efibootmgr_status"] = int(match.group(1))
    if s.args.windows_last:
        # efibootmgr puts a new entry first. With the Windows Boot Manager
        # stand-in on the disk itself (same-disk scenarios) the firmware
        # would then start it, not the medium: move the entry to the end of
        # the boot order, as if the person had chosen the stick in the
        # firmware's boot menu. The entry itself is what the tests compare.
        s.type_line(
            "o=$(efibootmgr | sed -n 's/^BootOrder: //p' | tr , '\\n' | grep -v \"^$(efibootmgr | sed -n "
            "'s/^Boot\\([0-9A-F]*\\)\\* Windows Boot Manager.*/\\1/p')$\" | paste -sd,); "
            "efibootmgr -o \"$o,$(efibootmgr | sed -n 's/^Boot\\([0-9A-F]*\\)\\* Windows Boot Manager.*/\\1/p')\" "
            "> /dev/ttyS0 2>&1; echo efibootmgr-order-status-$? > /dev/ttyS0"
        )
        match = s.wait(r"efibootmgr-order-status-(\d+)", 60, "efibootmgr -o")
        facts["efibootmgr_order_status"] = int(match.group(1))
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


def beside_phrase(serial: str) -> str:
    """The phrase that confirms installing beside Windows (ADR-0029),
    computed independently of the installer: "BESIDE" and the end of the
    serial number, as the erase phrase is "ERASE" and the same end."""
    return "BESIDE " + isovm.confirmation_phrase(serial).split(" ", 1)[1]


# Where the power is cut in same-disk-cut: a state line or a line of bootc's
# output on the installer's screen, mirrored to the serial port.
CUTS = {
    "record": r"TUR-INSTALLER: beside step=record ",
    "partition": r"TUR-INSTALLER: beside step=partition ",
    "partitioned": r"TUR-INSTALLER: beside step=partitioned ",
    "filesystem": r"TUR-INSTALLER: beside step=filesystem ",
    "deploy": r"TUR-INSTALLER: beside step=deploy ",
    "bootloader": r"Installing bootloader via bootupd",
    "deployed": r"TUR-INSTALLER: beside step=deployed ",
    "boot-menu": r"TUR-INSTALLER: beside step=boot-menu ",
    "check": r"TUR-INSTALLER: beside step=check",
}


def beside_to_install(s: Session) -> dict:
    """From the start of the medium to the start of an installation beside
    Windows on the main disk. Returns the main disk as listed."""
    facts = s.result["facts"]
    s.to_welcome()
    target = s.to_disks()
    s.shot("disks")
    beside = [isovm.fields(state) for state in s.transcript.states() if state.startswith("beside ")]
    facts["beside_as_listed"] = beside
    mine = next((item for item in reversed(beside) if target and item.get("name") == target.get("name")), None)
    facts["target_beside"] = mine
    if not target or target.get("status") != "ok" or not mine or mine.get("possible") != "yes":
        raise Stop(f"the Windows disk is not offered for installing beside Windows: {target} {mine}")
    s.type_line(target["index"])
    s.wait_prompt("mode")
    s.shot("mode")
    s.type_line("1")
    match = s.wait(r'TUR-INSTALLER: mode chosen=(\w+)', 60, "the choice of beside Windows")
    facts["mode_chosen"] = match.group(1)
    s.wait_prompt("default")
    s.shot("default-system")
    s.type_line("1" if s.args.default_system == "tur" else "2")
    match = s.wait(r'TUR-INSTALLER: default chosen=(\w+)', 60, "the choice of the default system")
    facts["default_chosen"] = match.group(1)
    match = s.wait(r'TUR-INSTALLER: confirm name=\S+ by_id=(\S+) phrase="([^"]*)" mode=beside[^\n]*', 60,
                   "the beside-Windows confirmation")
    s.wait_prompt("confirm")
    expected = beside_phrase(DISK_SERIAL)
    facts["confirmation"] = {
        "by_id": match.group(1),
        "phrase_shown": match.group(2),
        "phrase_computed_by_the_test": expected,
        "phrase_matches": match.group(2) == expected,
        "line": match.group(0),
    }
    s.shot("confirm-beside")
    # The erase phrase of the same disk must not confirm this.
    erase = isovm.confirmation_phrase(DISK_SERIAL)
    s.type_line(erase)
    match = s.wait(r"TUR-INSTALLER: confirm result=(\w+)(?: code=(\S+))?", 60, "the answer to the erase phrase")
    facts["erase_phrase_on_beside"] = {"typed": erase, "result": match.group(1), "code": match.group(2)}
    if match.group(1) != "mismatch":
        raise Stop("the erase phrase was accepted for installing beside Windows")
    time.sleep(1.5)
    s.wait_prompt("confirm")
    s.type_line(expected)
    s.wait(r"TUR-INSTALLER: confirm result=accepted mode=beside", 60, "the confirmation to be accepted")
    s.wait(r"TUR-INSTALLER: beside revalidated ", 120, "the disk to be looked at again")
    return target


def collect_beside(s: Session) -> None:
    facts = s.result["facts"]
    facts["beside_states"] = [state for state in s.transcript.states() if state.startswith("beside ")]


def scenario_same_disk(s: Session) -> None:
    facts = s.result["facts"]
    beside_to_install(s)
    match = s.wait(r'TUR-INSTALLER: installing device=(\S+) command="([^"]*)"', 300, "the installation to start")
    facts["install_device"], facts["install_command"] = match.group(1), match.group(2)
    install_started = s.elapsed()
    time.sleep(20)
    s.shot("installing")
    match = s.wait(r"TUR-INSTALLER: install-finished status=(\d+) seconds=(\d+)", 2400, "the installation to finish")
    facts["install_status"], facts["install_seconds"] = int(match.group(1)), int(match.group(2))
    facts["install_seconds_seen_by_host"] = round(s.elapsed() - install_started, 1)
    match = s.wait(r"TUR-INSTALLER: result=(success|failed)\b[^\n]*", 600, "the result")
    facts["result"] = match.group(1)
    facts["result_line"] = match.group(0)
    collect_beside(s)
    s.step(f"installer result: {match.group(0)}")
    s.wait_prompt("finished" if facts["result"] == "success" else "failed", 60)
    s.shot("done" if facts["result"] == "success" else "failed")
    if facts["result"] != "success":
        raise Stop("the installer reported a failure")
    if s.args.dualboot_after:
        dualboot_after(s)
        return
    s.type_line("", "nothing")
    facts["powered_off_by_installer"] = s.wait_power_off()
    if not facts["powered_off_by_installer"]:
        s.machine.quit()


def scenario_same_disk_cut(s: Session) -> None:
    facts = s.result["facts"]
    where = s.args.cut_at
    facts["cut"] = {"at": where, "pattern": CUTS[where], "delay_seconds": s.args.cut_delay}
    beside_to_install(s)
    s.wait(CUTS[where], 2400, f"the installation to reach {where}")
    reached = s.elapsed()
    if s.args.cut_delay:
        time.sleep(s.args.cut_delay)
    # A power cut: the machine is frozen at once and QEMU killed. Nothing in
    # the guest is told; whatever its page cache held is lost.
    try:
        s.qmp.command("stop")
    except Exception as error:  # the kill below is what counts
        s.say(f"QMP stop failed: {error}")
    s.machine.kill()
    facts["cut"].update({"reached_at_seconds": reached, "cut_at_seconds": s.elapsed()})
    s.transcript.pump()
    collect_beside(s)
    facts["cut"]["last_states"] = s.transcript.states()[-8:]
    facts["cut"]["last_screen_lines"] = s.transcript.screen_lines()[-6:]
    s.step(f"power cut at {where} (+{s.args.cut_delay} s); last state lines: {facts['cut']['last_states'][-3:]}")


def dualboot_after(s: Session) -> None:
    """tur-dualboot run as root from the installer's command line after the
    installation, as an administrator would run it with pkexec on TUR."""
    facts = s.result["facts"]
    s.type_line("shell")
    s.wait(r"TUR-INSTALLER: shell=started", 60, "the command line")
    time.sleep(2)
    for number, command in enumerate(("status", "default windows", "status", "default tur", "status")):
        s.type_line(f"tur-dualboot {command} > /dev/ttyS0 2>&1; echo dualboot-{number}-$? > /dev/ttyS0")
        match = s.wait(rf"dualboot-{number}-(\d+)", 60, f"tur-dualboot {command}")
        facts.setdefault("dualboot", []).append({"command": command, "status": int(match.group(1))})
    s.shot("dualboot")
    s.type_line("tur-dualboot restart-windows > /dev/ttyS0 2>&1")
    # The machine restarts; the firmware starts BootNext's entry, once.
    match = s.wait(r"TUR-WINDOWS-STANDIN: started file=(\S+)", 300, "the stand-in for Windows Boot Manager")
    facts["restart_windows"] = {"standin_file": match.group(1)}
    found = s.wait(r"TUR-WINDOWS-STANDIN: partition=(\d+) guid=([0-9A-Fa-f-]{36})", 30, "the stand-in's partition")
    facts["restart_windows"]["standin_partition_uuid"] = found.group(2).upper()
    facts["restart_windows"]["powered_off_by_standin"] = s.wait_power_off(60)
    if not facts["restart_windows"]["powered_off_by_standin"]:
        s.machine.quit()
    facts["dualboot_lines"] = [line.strip() for line in s.transcript.text.split("\n")
                               if line.strip().startswith(("default=", "tur_entry=", "windows_entry=", "boot_order=",
                                                           "boot_next=", "tur-dualboot:"))]


def scenario_shrink(s: Session) -> None:
    facts = s.result["facts"]
    s.to_welcome()
    target = s.to_disks()
    s.shot("disks-shrink")
    shrink = [isovm.fields(state) for state in s.transcript.states() if state.startswith("shrink index=")]
    facts["shrink_as_listed"] = shrink
    mine = next((item for item in reversed(shrink) if target and item.get("name") == target.get("name")), None)
    facts["target_shrink"] = mine
    if not target or not mine or mine.get("possible") != "yes":
        raise Stop(f"the Windows disk is not offered for shrinking: {target} {mine}")
    s.type_line(target["index"])
    s.wait_prompt("mode")
    s.shot("mode-shrink")
    s.type_line("3")
    s.wait(r"TUR-INSTALLER: mode chosen=shrink", 60, "the choice of shrinking")
    s.wait_prompt("size")
    s.shot("size")
    s.type_line(str(s.args.tur_gib))
    match = s.wait(r"TUR-INSTALLER: size chosen=(\S+)", 60, "the size")
    facts["size_chosen"] = match.group(1)
    s.wait_prompt("default")
    s.type_line("1" if s.args.default_system == "tur" else "2")
    s.wait(r"TUR-INSTALLER: default chosen=", 60, "the default system")
    match = s.wait(r'TUR-INSTALLER: confirm name=\S+ by_id=(\S+) phrase="([^"]*)" mode=shrink[^\n]*', 60,
                   "the shrink confirmation")
    s.wait_prompt("confirm")
    expected = "SHRINK " + isovm.confirmation_phrase(DISK_SERIAL).split(" ", 1)[1]
    facts["confirmation"] = {"phrase_shown": match.group(2), "phrase_computed_by_the_test": expected,
                             "phrase_matches": match.group(2) == expected, "line": match.group(0)}
    s.shot("confirm-shrink")
    refusals = []
    for wrong in (isovm.confirmation_phrase(DISK_SERIAL), beside_phrase(DISK_SERIAL)):
        s.type_line(wrong)
        result = s.wait(r"TUR-INSTALLER: confirm result=(\w+)", 60, "the answer to a wrong phrase")
        refusals.append({"typed": wrong, "result": result.group(1)})
        time.sleep(1.5)
        s.wait_prompt("confirm")
    facts["wrong_phrases"] = refusals
    if any(item["result"] != "mismatch" for item in refusals):
        raise Stop(f"a wrong phrase was accepted: {refusals}")
    s.type_line(expected)
    s.wait(r"TUR-INSTALLER: confirm result=accepted mode=shrink", 60, "the confirmation")
    s.wait(r"TUR-INSTALLER: shrink revalidated ", 300, "the disk to be looked at again")
    match = s.wait(r"TUR-INSTALLER: shrink result=(\w+)[^\n]*", 1800, "the shrink")
    facts["shrink_line"] = match.group(0)
    match = s.wait(r"TUR-INSTALLER: result=(success|failed)\b[^\n]*", 2400, "the result")
    facts["result"] = match.group(1)
    facts["result_line"] = match.group(0)
    facts["shrink_states"] = [state for state in s.transcript.states() if state.startswith(("shrink ", "beside "))]
    s.wait_prompt("finished" if facts["result"] == "success" else "failed", 60)
    s.shot("done-shrink" if facts["result"] == "success" else "failed")
    if facts["result"] != "success":
        raise Stop("the installer reported a failure")
    s.type_line("", "nothing")
    facts["powered_off_by_installer"] = s.wait_power_off()
    if not facts["powered_off_by_installer"]:
        s.machine.quit()


def scenario_beside_refused(s: Session) -> None:
    facts = s.result["facts"]
    disks = [s.args.disk] + [item.split("=", 1)[0] for item in s.args.extra_disk]
    facts["disk_sha256_before"] = {path: isovm.sha256(path) for path in disks}
    s.to_welcome()
    s.to_disks()
    s.shot("disks-no-room-beside")
    beside = [isovm.fields(state) for state in s.transcript.states() if state.startswith("beside ")]
    facts["beside_as_listed"] = beside
    facts["shrink_as_listed"] = [isovm.fields(state) for state in s.transcript.states() if state.startswith("shrink index=")]
    for disk in facts.get("disks", []):
        if disk.get("status") != "ok" or not disk.get("index", "").isdigit():
            continue
        s.type_line(disk["index"])
        match = s.wait(r"TUR-INSTALLER: (prompt|waiting) screen=(mode|confirm)\b", 60, "the next screen")
        facts.setdefault("next_screen", {})[disk.get("serial", "?")] = match.group(2)
        time.sleep(1.0)
        if match.group(2) == "mode":
            raise Stop(f"installing beside Windows was offered on {disk.get('serial')}")
        s.type_line("", "nothing")
        s.wait(r"TUR-INSTALLER: confirm result=back", 60, "going back")
        s.wait_prompt("disks")
    s.type_line("off")
    s.wait(r"TUR-INSTALLER: power-off", 60, "the power-off")
    facts["powered_off_by_installer"] = s.wait_power_off()
    if not facts["powered_off_by_installer"]:
        s.machine.quit()
    facts["installing_seen"] = bool(s.transcript.search(r"TUR-INSTALLER: installing "))
    facts["disk_sha256_after"] = {path: isovm.sha256(path) for path in disks}
    facts["disks_unchanged"] = facts["disk_sha256_before"] == facts["disk_sha256_after"]


SCENARIOS = {
    "same-disk": scenario_same_disk,
    "same-disk-cut": scenario_same_disk_cut,
    "beside-refused": scenario_beside_refused,
    "shrink": scenario_shrink,
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
    ap.add_argument("--windows-serial", default=WINDOWS_SERIAL, help="windows-setup: the disk of the Windows entry")
    ap.add_argument("--windows-partition", default="1", help="windows-setup: its EFI system partition's number")
    ap.add_argument("--windows-last", action="store_true", help="windows-setup: put the entry last in the boot order")
    ap.add_argument("--tur-gib", type=int, default=26, help="shrink: the room for TUR in GiB")
    ap.add_argument("--default-system", choices=["tur", "windows"], default="tur",
                    help="same-disk: the system the computer starts by default")
    ap.add_argument("--dualboot-after", action="store_true",
                    help="same-disk: tur-dualboot default and restart-windows after the installation")
    ap.add_argument("--cut-at", choices=sorted(CUTS), default="partition", help="same-disk-cut: where to cut the power")
    ap.add_argument("--cut-delay", type=float, default=0.0, help="same-disk-cut: seconds after reaching --cut-at")
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
