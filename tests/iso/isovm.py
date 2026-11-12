#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Virtual-machine plumbing for the installer-medium tests.

Builds on tests/boot/vm.py (QEMU, QMP, screenshots) and adds what an
installation needs and the boot tests do not:

- `InstallMachine`: the boot tests' machine with the installer medium attached,
  as a USB stick or as an optical drive, and with **no network device at
  all**. Optionally OVMF with Secure Boot enforced and Microsoft's keys
  enrolled.
- `Keyboard`: types on the virtual machine's keyboard through QMP, the way a
  person types on the first console.
- `Transcript`: follows the serial log for what the installer wrote there.
  The installer mirrors every line of its screen to the first serial port and
  adds state lines; nothing is ever sent to the installer over the serial
  port.

Nothing here decides whether a test passed. Run in CI only (ADR-0002).
"""

from __future__ import annotations

import hashlib
import os
import pathlib
import re
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent / "boot"))
import vm  # noqa: E402

STATE = "TUR-INSTALLER: "
SCREEN = "TUR-INSTALLER| "
SECURE_BOOT_FIRMWARE = [
    ("/usr/share/OVMF/OVMF_CODE_4M.secboot.fd", "/usr/share/OVMF/OVMF_VARS_4M.ms.fd"),
    ("/usr/share/OVMF/OVMF_CODE.secboot.fd", "/usr/share/OVMF/OVMF_VARS.ms.fd"),
]


class InstallMachine(vm.Machine):
    """The boot tests' machine, with an installer medium and without a network."""

    def __init__(self, *, medium: str | None = None, medium_as: str = "usb", secure_boot: bool = False, **kwargs):
        super().__init__(**kwargs)
        if medium_as not in ("usb", "cdrom"):
            raise ValueError("medium_as must be usb or cdrom")
        self.medium, self.medium_as, self.secure_boot = medium, medium_as, secure_boot
        if secure_boot:
            for code, variables in SECURE_BOOT_FIRMWARE:
                if os.path.exists(code) and os.path.exists(variables):
                    self.code, self.variables_template = code, variables
                    break
            else:
                raise RuntimeError("no OVMF build with Secure Boot and enrolled keys was found")

    def command(self) -> list[str]:
        base = super().command()
        # The program, then pairs of an option and its value.
        command: list[str] = [base[0]]
        index = 1
        while index < len(base):
            flag, value = base[index], base[index + 1] if index + 1 < len(base) else ""
            # The installer must work with no network: remove the network card
            # altogether, and tell QEMU not to add its default one.
            if flag in ("-netdev", "-device") and ("id=net0" in value or "netdev=net0" in value):
                index += 2
                continue
            if flag == "-machine" and self.secure_boot:
                value += ",smm=on"
            if flag == "-device" and value.startswith("virtio-blk-pci,drive=disk0"):
                # The medium starts first when it is present; the disk otherwise.
                value += ",bootindex=1"
            command += [flag, value]
            index += 2
        command += ["-nic", "none"]
        if self.secure_boot:
            command += ["-global", "driver=cfi.pflash01,property=secure,value=on"]
        if self.medium:
            command += ["-drive", f"if=none,id=medium,format=raw,readonly=on,file={self.medium}"]
            if self.medium_as == "usb":
                command += [
                    "-device", "qemu-xhci,id=xhci",
                    "-device", "usb-storage,bus=xhci.0,drive=medium,removable=on,bootindex=0,serial=TURINSTALLERMEDIUM",
                ]
            else:
                command += ["-device", "ide-cd,bus=ide.0,drive=medium,bootindex=0"]
        return command


# Characters a person types in these tests, as QEMU key codes.
PLAIN = {
    " ": "spc", "-": "minus", "=": "equal", ".": "dot", ",": "comma", "/": "slash", ";": "semicolon",
    "'": "apostrophe", "\n": "ret",
}
SHIFTED = {
    "_": "minus", "+": "equal", ">": "dot", "<": "comma", "?": "slash", ":": "semicolon", '"': "apostrophe",
    "!": "1", "@": "2", "#": "3", "$": "4", "%": "5", "^": "6", "&": "7", "*": "8", "(": "9", ")": "0",
}


class Keyboard:
    def __init__(self, qmp: vm.Qmp):
        self.qmp = qmp

    def press(self, *codes: str) -> None:
        self.qmp.command(
            "send-key",
            keys=[{"type": "qcode", "data": code} for code in codes],
            **{"hold-time": 60},
        )
        time.sleep(0.14)

    def text(self, text: str) -> None:
        for character in text:
            if character in PLAIN:
                self.press(PLAIN[character])
            elif character in SHIFTED:
                self.press("shift", SHIFTED[character])
            elif character.isdigit() or (character.isascii() and character.islower()):
                self.press(character)
            elif character.isascii() and character.isupper():
                self.press("shift", character.lower())
            else:
                raise ValueError(f"no key for {character!r}")

    def line(self, text: str = "") -> None:
        """Type a line and press Enter."""
        self.text(text)
        time.sleep(0.3)
        self.press("ret")


class Transcript:
    """What the serial port carried, cleaned of terminal control sequences."""

    def __init__(self, path: pathlib.Path):
        self.path = path
        self.offset = 0
        self.pending = ""
        self.text = ""

    def pump(self) -> None:
        if not self.path.exists():
            return
        with open(self.path, "rb") as stream:
            stream.seek(self.offset)
            data = stream.read()
        self.offset += len(data)
        self.pending += data.decode("utf-8", errors="replace")
        complete, _, self.pending = self.pending.rpartition("\n")
        if complete:
            self.text += vm.clean(complete + "\n")

    def position(self) -> int:
        return len(self.text)

    def search(self, pattern: str, since: int = 0) -> re.Match | None:
        return re.search(pattern, self.text[since:], re.M)

    def states(self, since: int = 0) -> list[str]:
        """The installer's state lines, without the prefix."""
        found = []
        for line in self.text[since:].split("\n"):
            if STATE in line:
                found.append(line[line.index(STATE) + len(STATE):].strip())
        return found

    def screen_lines(self, since: int = 0) -> list[str]:
        """The lines the installer put on the screen."""
        found = []
        for line in self.text[since:].split("\n"):
            if SCREEN in line:
                found.append(line[line.index(SCREEN) + len(SCREEN):].rstrip())
            elif line.rstrip().endswith(SCREEN.rstrip()):
                found.append("")
        return found

    def tagged(self, tag: str) -> list[str]:
        """Lines written with another prefix, such as TUR-INSTALLER-LSBLK| ."""
        prefix = f"{tag}| "
        return [line[line.index(prefix) + len(prefix):] for line in self.text.split("\n") if prefix in line]


def fields(state: str) -> dict[str, str]:
    """key=value pairs of a state line; values may be quoted."""
    return {key: quoted if quoted else plain for key, quoted, plain in re.findall(r'(\w+)=(?:"([^"]*)"|(\S*))', state)}


def sha256(path: pathlib.Path | str) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        for chunk in iter(lambda: stream.read(1 << 22), b""):
            digest.update(chunk)
    return digest.hexdigest()


def confirmation_phrase(serial: str) -> str:
    """The phrase the installer asks for, computed independently of it: the
    rule of installer/tur-installer-core (plan.rs, confirmation_phrase)."""
    cleaned = "".join(character for character in serial if character.isascii() and character.isalnum()).upper()
    return f"ERASE {cleaned[-8:]}"
