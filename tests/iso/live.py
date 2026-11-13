#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Start a virtual machine from the TUR installer medium into its live
session, and install TUR with the graphical installer the way a person does
with a keyboard.

    tests/iso/live.py install --medium tur-installer.iso --disk disk.raw \
        --work WORK_DIR --out OUT_DIR [--medium-as usb|cdrom] [--secure-boot] \
        [--extra-disk windows.raw=windows-disk-a]

The machine is tests/iso/install.py's (QEMU/KVM, OVMF, a virtio disk with the
serial number "tur-boot-test", the medium, no network device), with more
memory for the desktop and with a reboot turned into a power-off, so that
"Restart now" ends the run as removing the stick would.

How the installer is driven: QMP `send-key` only, as a person types. The
installer is keyboard-navigable (Tab moves, Space presses), and it writes
which control has the keyboard to its state lines ("TUR-WIZARD: focus
item=NAME"), which a service of the live session copies to the serial port;
so this driver presses Tab until the control it wants has the keyboard, then
Space. It never sends anything to the guest over the serial port and never
clicks: nothing here could not be done by a person with a keyboard. The
installation engine's own state lines ("TUR-INSTALLER: ...") are the text
installer's, from the same engine.

What the run does:

  1. Starts the medium with the boot menu's default entry: the live session.
     Waits for the desktop and the installer's window; screenshot.
  2. Welcome, then "This computer": waits for the checks.
  3. "Where to install": every disk listed; the medium is listed as the
     medium and Tab never reaches it.
  4. If a Windows-like disk (serial windows-disk-a) is attached: chooses it,
     goes on to the confirmation, which must show the Windows warning; types
     a wrong phrase, then the right phrase in lower case, and the erase button
     must stay unavailable for both; goes back.
  5. Chooses the disk "tur-boot-test", goes on to the confirmation; types the
     phrase; presses Enter, which must erase nothing; presses "Erase disk and
     install"; follows the stages; screenshot of every screen.
  6. On the result, "Restart now". QEMU stops at the reset.

The scenario "beside" (ADR-0028) has one disk, laid out as Windows leaves it
after Shrink Volume (tests/iso/make-windows-shared-disk.sh), as the main disk
"tur-boot-test". It chooses that disk, then "Use the free space beside
Windows", goes on to the confirmation, types the disk's erase phrase (the
install button must stay unavailable), then the "BESIDE" phrase, presses
Enter (nothing may happen), then "Install beside Windows", and follows the
installation to its end as "install" does.

It writes OUT_DIR/result.json (the facts tests/iso/report.py reads, in the
same shape as tests/iso/install.py's, plus the live session's own),
OUT_DIR/serial.log and numbered screenshots, and exits 0 if the run reached
its end. Whether that is a pass is decided by tests/iso/report.py.

Run in CI only (ADR-0002): never on a shared or production machine.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import efivars  # noqa: E402
import isovm  # noqa: E402
from isovm import vm  # noqa: E402

DISK_SERIAL = "tur-boot-test"
WINDOWS_SERIAL = "windows-disk-a"
WIZARD = "TUR-WIZARD: "


class Stop(Exception):
    """The run cannot continue."""


class LiveMachine(isovm.InstallMachine):
    """The installer machine; a reboot ends QEMU instead of starting again."""

    def command(self) -> list[str]:
        return super().command() + ["-action", "reboot=shutdown"]


class Run:
    def __init__(self, args):
        self.args = args
        self.out = pathlib.Path(args.out).resolve()
        self.work = pathlib.Path(args.work).resolve()
        self.host = self.out / "host"
        self.host.mkdir(parents=True, exist_ok=True)
        self.work.mkdir(parents=True, exist_ok=True)
        self.serial_log = self.out / "serial.log"
        self.machine = LiveMachine(
            disk=args.disk,
            work=self.work,
            serial_log=self.serial_log,
            report_tar=self.work / "live-unused-report.tar",
            qemu_log=self.host / "qemu.log",
            name="tur-iso-live",
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
        self.shots = 0
        self.started = time.monotonic()
        self.result: dict = {
            "scenario": "live-" + args.scenario,
            "frontend": "graphical",
            "outcome": "not-started",
            "medium": str(args.medium),
            "medium_attached_as": args.medium_as,
            "secure_boot_firmware": bool(args.secure_boot),
            "network_devices": 0,
            "boot_order_from_qemu": not args.no_bootindex,
            "extra_disks": list(args.extra_disk),
            "disk": str(args.disk),
            "disk_serial": DISK_SERIAL,
            "guest": {"cpus": args.cpus, "memory_mib": args.memory},
            "qemu_version": self.machine.version,
            "firmware": self.machine.code,
            "command": self.machine.command(),
            "input": "QMP send-key only: Tab to move, Space to press, letters to type",
            "steps": [],
            "screenshots": [],
            "facts": {"live": {}},
        }
        self.qmp = None
        self.keyboard = None

    # ---- plumbing -------------------------------------------------------------------

    def elapsed(self) -> float:
        return round(time.monotonic() - self.started, 1)

    def say(self, text: str) -> None:
        print(f"[{self.elapsed():7.1f}s] {text}", flush=True)

    def step(self, text: str) -> None:
        self.result["steps"].append({"at_seconds": self.elapsed(), "what": text})
        self.say(text)

    @property
    def live(self) -> dict:
        return self.result["facts"]["live"]

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

    def pump(self) -> str:
        self.transcript.pump()
        return self.transcript.text

    def wait(self, pattern: str, timeout: float, what: str, since: int = 0) -> re.Match:
        deadline = time.monotonic() + timeout
        while True:
            text = self.pump()
            match = re.search(pattern, text[since:], re.M)
            if match:
                return match
            if not self.machine.alive():
                raise Stop(f"QEMU exited while waiting for {what}")
            if time.monotonic() > deadline:
                raise Stop(f"timeout after {timeout:.0f} s waiting for {what}")
            time.sleep(0.4)
            self.qmp.drain()

    def wizard_lines(self, since: int = 0) -> list[str]:
        text = self.pump()[since:]
        return [line[line.index(WIZARD) + len(WIZARD):].strip() for line in text.split("\n") if WIZARD in line]

    def focus(self) -> str:
        """The control that has the keyboard, as the installer last said."""
        for line in reversed(self.wizard_lines()):
            match = re.match(r"focus item=(\S+)", line)
            if match:
                return match.group(1)
        return ""

    def screen_ready(self, name: str, timeout: float = 120) -> None:
        self.wait(rf"TUR-WIZARD: screen-ready name={name}\b", timeout, f"the {name} screen")
        time.sleep(1.5)

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

    def key(self, *codes: str) -> None:
        self.keyboard.press(*codes)

    def tab_to(self, name: str, limit: int = 45) -> list[str]:
        """Press Tab until `name` has the keyboard. Returns what Tab passed."""
        passed = []
        for _ in range(limit):
            current = self.focus()
            if current == name:
                return passed
            before = len(self.wizard_lines())
            self.key("tab")
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline and len(self.wizard_lines()) == before:
                time.sleep(0.1)
            passed.append(self.focus())
        raise Stop(f"Tab never reached {name}; it passed {passed}")

    def press(self, name: str) -> None:
        self.tab_to(name)
        self.step(f"Space on {name}")
        self.key("spc")
        time.sleep(0.6)

    def type(self, text: str, shown: str | None = None) -> None:
        self.step(f"typed {shown if shown is not None else repr(text)}")
        self.keyboard.text(text)
        time.sleep(0.5)

    def clear_field(self) -> None:
        self.key("ctrl", "a")
        self.key("backspace")
        time.sleep(0.4)

    def last_wizard(self, pattern: str) -> re.Match | None:
        for line in reversed(self.wizard_lines()):
            match = re.match(pattern, line)
            if match:
                return match
        return None

    def wait_power_off(self, timeout: float = 240) -> bool:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if not self.machine.alive():
                self.machine.collect()
                return True
            time.sleep(0.5)
        return False

    def firmware_entries(self) -> None:
        for label, path in (("before", self.machine.variables_template), ("after", self.machine.variables)):
            try:
                entries = efivars.boot_entries(pathlib.Path(path))
            except (OSError, ValueError) as error:
                entries = {"error": str(error), "file": str(path)}
            (self.host / f"efivars-{label}.json").write_text(json.dumps(entries, indent=2) + "\n")
            self.result.setdefault("firmware_boot_entries", {})[label] = entries

    # ---- facts from the engine's state lines (the text installer's) ---------------------

    def engine_facts(self) -> None:
        facts = self.result["facts"]
        latest: dict[str, dict] = {}
        for state in self.transcript.states():
            fields = isovm.fields(state)
            if state.startswith("start "):
                facts["start"] = fields
            elif state.startswith("cmdline "):
                facts["kernel_command_line"] = state[len("cmdline "):]
            elif state.startswith("live-medium "):
                facts["live_medium"] = state[len("live-medium "):]
            elif state.startswith("check "):
                facts.setdefault("checks", {}).update(fields)
            elif state.startswith("disk "):
                latest[fields.get("name", "?")] = fields
            elif state.startswith("disks "):
                facts["disks_selectable"] = fields.get("selectable")
            elif state.startswith("root-filesystem "):
                facts["root_filesystem"] = fields
            elif state.startswith("log-kept "):
                facts["log_kept"] = fields
        facts["disks"] = list(latest.values())
        target = next((disk for disk in latest.values() if disk.get("serial") == DISK_SERIAL), None)
        facts["target_as_listed"] = target

    # ---- the run ---------------------------------------------------------------------

    def to_live_desktop(self) -> None:
        live = self.live
        match = self.wait(r"TUR-LIVE: session-starting", 420, "the live session (graphical.target)")
        live["session_service_seconds"] = self.elapsed()
        self.step(f"live session starting: {match.group(0)}")
        self.wait(r"TUR-WIZARD: start ", 420, "the installer to start in the session")
        live["wizard_started_seconds"] = self.elapsed()
        self.result["facts"]["installer_started_seconds"] = live["wizard_started_seconds"]
        start = self.last_wizard(r"start (.*)")
        live["wizard_start"] = start.group(1) if start else None
        self.wait(r"TUR-WIZARD: window shown", 120, "the installer's window")
        shown = self.last_wizard(r"window shown (.*)")
        live["window"] = shown.group(1) if shown else None
        try:
            self.wait(r"TUR-WIZARD: window active=true", 60, "the installer's window to be active")
            live["window_active"] = True
        except Stop:
            live["window_active"] = False
            self.step("the window did not report itself active; going on")
        self.screen_ready("welcome", 120)
        live["welcome_seconds"] = self.elapsed()
        self.result["facts"]["welcome_seconds"] = live["welcome_seconds"]
        # The desktop is drawn behind the window; give the panel and the
        # wallpaper a moment.
        time.sleep(4)
        self.shot("live-desktop")
        self.shot("welcome")

    def through_checks(self) -> None:
        self.press("continueButton")
        self.screen_ready("computer")
        match = self.wait(r"TUR-WIZARD: checks blocking=(\d+) payload=(\S+) secure_boot=(\S+)", 600, "the checks")
        self.live["checks"] = {"blocking": int(match.group(1)), "payload": match.group(2), "secure_boot": match.group(3)}
        time.sleep(2)
        self.shot("computer")
        if int(match.group(1)) != 0:
            raise Stop(f"a check stops the installation: {match.group(0)}")
        self.press("continueButton")

    def disk_screen(self) -> dict[str, dict]:
        since = len(self.pump())
        self.screen_ready("disk")
        self.wait(r"TUR-WIZARD: disks total=(\d+) selectable=(\d+)", 120, "the disks", since)
        listed = {}
        for line in self.wizard_lines(since):
            if line.startswith("disk-listed "):
                fields = isovm.fields(line)
                listed[fields.get("serial", "")] = fields
        time.sleep(1.5)
        return listed

    def to_confirm(self, name: str) -> int:
        """From the disk screen, choose `name` and go on to the confirmation."""
        self.press(f"disk-{name}")
        self.wait(rf"TUR-WIZARD: disk-chosen name={name}\b", 30, f"disk {name} to be chosen")
        self.press("continueButton")
        self.screen_ready("account")
        since = len(self.pump())
        self.press("continueButton")
        self.screen_ready("confirm")
        return since

    def phrase_state(self, length: int | None = None, after: int = 0, timeout: float = 15) -> dict:
        """What the installer said after the field held `length` characters.

        The state lines reach the serial port a moment after the keys: wait
        for the line that describes the whole text typed, not an earlier one."""
        deadline = time.monotonic() + timeout
        while True:
            match = None
            for line in reversed(self.wizard_lines()[after:]):
                match = re.match(r"phrase-typed length=(\d+) matches=(\w+) erase_enabled=(\w+)", line)
                if match:
                    break
            if match and (length is None or int(match.group(1)) == length):
                return {"length": int(match.group(1)), "matches": match.group(2), "erase_enabled": match.group(3)}
            if time.monotonic() > deadline:
                return {"length": int(match.group(1)), "matches": match.group(2), "erase_enabled": match.group(3),
                        "waited_for_length": length} if match else {"waited_for_length": length}
            time.sleep(0.3)

    def run_install(self) -> None:
        facts, live = self.result["facts"], self.live
        self.to_live_desktop()
        self.through_checks()

        listed = self.disk_screen()
        live["disks_listed"] = listed
        self.shot("disks")
        medium = next((disk for disk in listed.values() if disk.get("status") == "installer-medium"), None)
        live["medium_as_listed"] = medium
        # Tab round the whole screen: the medium must never have the keyboard.
        seen = []
        for _ in range(16):
            self.key("tab")
            time.sleep(0.35)
            seen.append(self.focus())
        live["tab_stops_on_the_disk_screen"] = seen
        live["medium_reached_by_tab"] = bool(medium) and f"disk-{medium.get('name')}" in seen

        windows = listed.get(WINDOWS_SERIAL)
        live["windows_disk_as_listed"] = windows
        if windows is not None:
            since = self.to_confirm(windows["name"])
            shown = self.last_wizard(r"confirm-shown (.*)")
            live["confirm_windows"] = isovm.fields(shown.group(1)) if shown else None
            time.sleep(1)
            self.shot("confirm-windows-disk")
            if self.focus() != "phraseField":
                self.tab_to("phraseField")
            attempts = []
            for label, text in (("a wrong phrase", "ERASE WRONG123"),
                                ("the right phrase in lower case", isovm.confirmation_phrase(WINDOWS_SERIAL).lower())):
                before = len(self.wizard_lines())
                self.type(text)
                time.sleep(0.8)
                attempts.append(dict(self.phrase_state(len(text), before), what=label, typed=text))
                if len(attempts) == 1:
                    self.shot("confirm-windows-wrong-phrase")
                self.clear_field()
            live["windows_phrase_attempts"] = attempts
            live["windows_erase_requested"] = bool(re.search(r"TUR-WIZARD: (erase-confirmed|install-requested)",
                                                             self.pump()[since:]))
            self.press("backButton")
            listed = self.disk_screen()

        target = listed.get(DISK_SERIAL)
        if not target or target.get("status") != "ok":
            raise Stop(f"the test disk is not offered: {target}")
        since = self.to_confirm(target["name"])
        shown = self.last_wizard(r"confirm-shown (.*)")
        confirm = isovm.fields(shown.group(1)) if shown else {}
        live["confirm_blank"] = confirm
        live["focus_on_confirm"] = self.focus()
        time.sleep(1)
        self.shot("confirm")
        phrase = isovm.confirmation_phrase(DISK_SERIAL)
        self.engine_facts()
        facts["confirmation"] = {
            "by_id": confirm.get("by_id"),
            "phrase_shown": confirm.get("phrase"),
            "phrase_computed_by_the_test": phrase,
            "phrase_matches": confirm.get("phrase") == phrase,
        }
        if self.focus() != "phraseField":
            self.tab_to("phraseField")
        before = len(self.wizard_lines())
        self.type(phrase)
        time.sleep(0.8)
        live["phrase_typed"] = self.phrase_state(len(phrase), before)
        self.shot("confirm-typed")
        # Enter in the field must not erase anything.
        self.step("Enter in the confirmation field")
        self.key("ret")
        time.sleep(3)
        live["enter_erased"] = bool(re.search(r"TUR-WIZARD: (erase-confirmed|install-requested)", self.pump()[since:]))
        if live["enter_erased"]:
            raise Stop("Enter in the confirmation field started the installation")

        self.press("eraseButton")
        match = self.wait(r'TUR-INSTALLER: installing device=(\S+) command="([^"]*)"', 300, "the installation to start", since)
        facts["install_device"], facts["install_command"] = match.group(1), match.group(2)
        install_started = self.elapsed()
        self.step(f"installation started: {match.group(2)}")
        self.screen_ready("install", 60)
        cancellable = self.last_wizard(r"stage name=revalidate cancellable=(\w+)")
        live["cancellable_while_revalidating"] = cancellable.group(1) if cancellable else None
        try:
            self.wait(r"TUR-WIZARD: stage name=copy cancellable=(\w+)", 600, "the copy stage", since)
            stage = self.last_wizard(r"stage name=copy cancellable=(\w+)")
            live["cancellable_while_copying"] = stage.group(1) if stage else None
            time.sleep(6)
            self.shot("installing")
        except Stop as error:
            self.step(f"no copy stage seen: {error}")
        match = self.wait(r"TUR-INSTALLER: install-finished status=(\d+) seconds=(\d+)", 2400, "the installation to finish",
                          since)
        facts["install_status"], facts["install_seconds"] = int(match.group(1)), int(match.group(2))
        facts["install_seconds_seen_by_host"] = round(self.elapsed() - install_started, 1)
        match = self.wait(r"TUR-INSTALLER: result=(success|failed)\b[^\n]*", 300, "the result", since)
        facts["result"] = match.group(1)
        facts["result_line"] = match.group(0)
        self.screen_ready("done", 120)
        stages = [line.split()[1].split("=", 1)[1] for line in self.wizard_lines(since) if line.startswith("stage name=")]
        live["stages_seen"] = stages
        shown = self.last_wizard(r"result-shown (.*)")
        live["result_shown"] = isovm.fields(shown.group(1)) if shown else None
        time.sleep(2)
        self.shot("done")
        self.engine_facts()
        if facts["result"] != "success":
            raise Stop("the installer reported a failure")
        self.press("restartButton")
        self.wait(r"TUR-WIZARD: power request=reboot", 30, "the restart to be asked for")
        facts["powered_off_by_installer"] = self.wait_power_off()
        live["restart_ended_qemu"] = facts["powered_off_by_installer"]
        self.step(f"the machine restarted (QEMU stopped at the reset): {facts['powered_off_by_installer']}")
        if not facts["powered_off_by_installer"]:
            self.machine.quit()

    def run_beside(self) -> None:
        facts, live = self.result["facts"], self.live
        self.to_live_desktop()
        self.through_checks()
        listed = self.disk_screen()
        live["disks_listed"] = listed
        target = listed.get(DISK_SERIAL)
        if not target or target.get("status") != "ok" or target.get("beside") != "possible":
            raise Stop(f"the Windows disk is not offered for installing beside Windows: {target}")
        self.press(f"disk-{target['name']}")
        self.wait(rf"TUR-WIZARD: disk-chosen name={target['name']}\b", 30, "the disk to be chosen")
        time.sleep(1)
        self.shot("disks-windows-chosen")
        self.press("besideWindowsOption")
        self.wait(r"TUR-WIZARD: mode-chosen mode=beside", 30, "beside Windows to be chosen")
        live["mode_chosen"] = "beside"
        time.sleep(1)
        self.shot("disks-beside-chosen")
        self.press("continueButton")
        self.screen_ready("account")
        since = len(self.pump())
        self.press("continueButton")
        self.screen_ready("confirm")
        shown = self.last_wizard(r"confirm-shown (.*)")
        confirm = isovm.fields(shown.group(1)) if shown else {}
        live["confirm_beside"] = confirm
        live["focus_on_confirm"] = self.focus()
        time.sleep(1)
        self.shot("confirm-beside")
        phrase = "BESIDE " + isovm.confirmation_phrase(DISK_SERIAL).split(" ", 1)[1]
        self.engine_facts()
        facts["confirmation"] = {
            "by_id": confirm.get("by_id"),
            "phrase_shown": confirm.get("phrase"),
            "phrase_computed_by_the_test": phrase,
            "phrase_matches": confirm.get("phrase") == phrase,
            "mode": confirm.get("mode"),
        }
        if self.focus() != "phraseField":
            self.tab_to("phraseField")
        erase = isovm.confirmation_phrase(DISK_SERIAL)
        before = len(self.wizard_lines())
        self.type(erase)
        time.sleep(0.8)
        live["erase_phrase_on_beside"] = dict(self.phrase_state(len(erase), before), typed=erase)
        self.clear_field()
        before = len(self.wizard_lines())
        self.type(phrase)
        time.sleep(0.8)
        live["phrase_typed"] = self.phrase_state(len(phrase), before)
        self.shot("confirm-beside-typed")
        self.step("Enter in the confirmation field")
        self.key("ret")
        time.sleep(3)
        live["enter_erased"] = bool(re.search(r"TUR-WIZARD: (erase-confirmed|install-requested)", self.pump()[since:]))
        if live["enter_erased"]:
            raise Stop("Enter in the confirmation field started the installation")
        self.press("eraseButton")
        match = self.wait(r'TUR-INSTALLER: installing device=(\S+) command="([^"]*)"', 600, "the installation to start", since)
        facts["install_device"], facts["install_command"] = match.group(1), match.group(2)
        install_started = self.elapsed()
        self.screen_ready("install", 60)
        try:
            self.wait(r"TUR-WIZARD: stage name=copy cancellable=(\w+)", 600, "the copy stage", since)
            time.sleep(6)
            self.shot("installing-beside")
        except Stop as error:
            self.step(f"no copy stage seen: {error}")
        match = self.wait(r"TUR-INSTALLER: install-finished status=(\d+) seconds=(\d+)", 2400,
                          "the installation to finish", since)
        facts["install_status"], facts["install_seconds"] = int(match.group(1)), int(match.group(2))
        facts["install_seconds_seen_by_host"] = round(self.elapsed() - install_started, 1)
        match = self.wait(r"TUR-INSTALLER: result=(success|failed)\b[^\n]*", 600, "the result", since)
        facts["result"] = match.group(1)
        facts["result_line"] = match.group(0)
        facts["beside_states"] = [state for state in self.transcript.states() if state.startswith("beside ")]
        self.screen_ready("done", 120)
        live["stages_seen"] = [line.split()[1].split("=", 1)[1] for line in self.wizard_lines(since)
                               if line.startswith("stage name=")]
        shown = self.last_wizard(r"result-shown (.*)")
        live["result_shown"] = isovm.fields(shown.group(1)) if shown else None
        time.sleep(2)
        self.shot("done-beside")
        self.engine_facts()
        if facts["result"] != "success":
            raise Stop("the installer reported a failure")
        self.press("restartButton")
        self.wait(r"TUR-WIZARD: power request=reboot", 30, "the restart to be asked for")
        facts["powered_off_by_installer"] = self.wait_power_off()
        if not facts["powered_off_by_installer"]:
            self.machine.quit()

    def finish(self, outcome: str) -> int:
        self.transcript.pump()
        self.machine.collect()
        if not self.machine.alive():
            self.firmware_entries()
        self.engine_facts()
        self.result["outcome"] = outcome
        self.result["elapsed_seconds"] = self.elapsed()
        self.result["events"] = [
            {"event": event["event"], "at_seconds": round(event["host_time"] - self.started, 1)}
            for event in self.machine.events
        ]
        self.result["installer_states"] = self.transcript.states()
        self.result["wizard_states"] = self.wizard_lines()
        (self.out / "wizard-transcript.txt").write_text("\n".join(self.wizard_lines()) + "\n")
        lsblk = self.transcript.tagged("TUR-INSTALLER-LSBLK")
        if lsblk:
            (self.out / "live-lsblk.json").write_text("\n".join(lsblk) + "\n")
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
        tail = [line for line in self.transcript.text.split("\n") if line.strip()][-150:]
        print(f"----- last {len(tail)} lines of the serial console -----")
        print("\n".join(tail))
        print("----- end of serial console -----", flush=True)
        return self.finish("stopped")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("scenario", choices=["install", "beside"])
    ap.add_argument("--medium", required=True)
    ap.add_argument("--disk", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--medium-as", choices=["usb", "cdrom"], default="usb")
    ap.add_argument("--secure-boot", action="store_true")
    ap.add_argument("--extra-disk", action="append", default=[], metavar="PATH=SERIAL")
    ap.add_argument("--no-bootindex", action="store_true")
    ap.add_argument("--variables-from")
    ap.add_argument("--cpus", type=int, default=2)
    ap.add_argument("--memory", type=int, default=6144, help="MiB; the live desktop and the installation together")
    ap.add_argument("--qemu", default="qemu-system-x86_64")
    args = ap.parse_args()

    run = Run(args)
    try:
        run.start()
        if args.scenario == "beside":
            run.run_beside()
        else:
            run.run_install()
    except (Stop, RuntimeError, ConnectionError) as error:
        return run.abort(str(error))
    if run.machine.variables.exists():
        (run.out / "host" / "OVMF_VARS.after.fd").write_bytes(run.machine.variables.read_bytes())
    return run.finish("completed")


if __name__ == "__main__":
    sys.exit(main())
