# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Windows paths and Linux paths, both ways, for one Windows environment.

A Windows environment is a Wine prefix: `drive_c` is C:, and `dosdevices`
maps the other letters. Folders of the person's home may be mapped into it
(C:\\users\\<name>\\Documents is a link to ~/Documents); those links are
what `leaves_environment` looks for.
"""

from __future__ import annotations

import getpass
import os
import pathlib
import re

DRIVE = re.compile(r"^([A-Za-z]):(?:[\\/]|$)")
WINVAR = re.compile(r"%([A-Za-z_()0-9]+)%")
PSVAR = re.compile(r"\$env:([A-Za-z_()0-9]+)", re.I)


class WinEnv:
    def __init__(self, prefix: str | None = None):
        chosen = prefix or os.environ.get("TUR_WINPREFIX") or os.environ.get("WINEPREFIX") or "~/.wine"
        self.prefix = pathlib.Path(chosen).expanduser()
        self.drive_c = self.prefix / "drive_c"
        self.user = getpass.getuser()
        self.drives: dict[str, str] = {"c": str(self.drive_c)}
        dos = self.prefix / "dosdevices"
        if dos.is_dir():
            for entry in sorted(dos.iterdir()):
                if re.fullmatch(r"[a-z]:", entry.name) and entry.name != "c:" and entry.is_symlink():
                    target = os.path.normpath(os.path.join(dos, os.readlink(entry)))
                    self.drives[entry.name[0]] = target
        users = self._spelled(str(self.drive_c), "Users")
        windows = self._spelled(str(self.drive_c), "Windows")
        profile = f"C:\\{users}\\{self.user}"
        self.vars = {
            "USERPROFILE": profile, "HOMEDRIVE": "C:", "HOMEPATH": f"\\{users}\\{self.user}",
            "APPDATA": profile + "\\AppData\\Roaming", "LOCALAPPDATA": profile + "\\AppData\\Local",
            "TEMP": profile + "\\AppData\\Local\\Temp", "TMP": profile + "\\AppData\\Local\\Temp",
            "SYSTEMROOT": f"C:\\{windows}", "WINDIR": f"C:\\{windows}", "SYSTEMDRIVE": "C:",
            "PROGRAMFILES": "C:\\Program Files", "PROGRAMDATA": "C:\\ProgramData",
            "PUBLIC": f"C:\\{users}\\Public", "USERNAME": self.user,
        }

    @staticmethod
    def _spelled(parent: str, name: str) -> str:
        """`name` as it is spelled in `parent` (Windows ignores case)."""
        if os.path.lexists(os.path.join(parent, name)):
            return name
        try:
            for entry in os.listdir(parent):
                if entry.lower() == name.lower():
                    return entry
        except OSError:
            pass
        return name

    def expand(self, text: str) -> str:
        """%VAR% and $env:VAR for the variables that name folders."""
        def sub(match):
            return self.vars.get(match.group(1).upper(), match.group(0))
        return PSVAR.sub(sub, WINVAR.sub(sub, text))

    def win_to_unix(self, path: str, cwd: str | None = None) -> str | None:
        """The Linux path a Windows path names, or None if it names none."""
        path = self.expand(path.strip().strip('"'))
        if not path:
            return None
        if path.upper() in ("NUL", "NUL:"):
            return "/dev/null"
        lowered = path.lower()
        if lowered.startswith("\\\\?\\unix\\") or lowered.startswith("//?/unix/"):
            return "/" + path[9:].replace("\\", "/")
        if path.startswith("\\\\") or path.startswith("//"):
            return None
        match = DRIVE.match(path)
        if match:
            root = self.drives.get(match.group(1).lower())
            if root is None:
                return None
            rest = path[2:]
        elif path.startswith(("\\", "/")) and "\\" in path:
            root, rest = str(self.drive_c), path
        elif path.startswith("/"):
            return path
        else:
            root, rest = cwd or os.getcwd(), path
        here = root
        for part in [p for p in re.split(r"[\\/]+", rest) if p]:
            if part == ".":
                continue
            if part == "..":
                here = os.path.dirname(here.rstrip("/")) or "/"
                continue
            here = os.path.join(here, self._spelled(here, part))
        return here

    def unix_to_win(self, path: str) -> str | None:
        """A Windows path for a Linux path, through C: when a mapped folder
        or drive_c reaches it, through another drive letter otherwise."""
        path = os.path.abspath(os.path.expanduser(path))
        if path == "/dev/null":
            return "NUL"
        best: tuple[int, str] | None = None
        for letter, root in self.drives.items():
            root = root.rstrip("/") or "/"
            if path == root or path.startswith(root.rstrip("/") + "/"):
                rest = path[len(root.rstrip("/")):].strip("/")
                score = len(root) + (10000 if letter == "c" else 0)
                if best is None or score > best[0]:
                    best = (score, f"{letter.upper()}:\\" + rest.replace("/", "\\"))
        if best and best[1].upper().startswith("C:"):
            return best[1]
        # A folder of the home mapped into the profile (Documents, ...).
        users = self._spelled(str(self.drive_c), "Users")
        profile = self.drive_c / users / self.user
        try:
            for entry in sorted(profile.iterdir()):
                if entry.is_symlink():
                    target = os.path.realpath(entry)
                    if path == target or path.startswith(target + "/"):
                        rest = path[len(target):].strip("/")
                        base = f"C:\\{users}\\{self.user}\\{entry.name}"
                        return base + ("\\" + rest.replace("/", "\\") if rest else "")
        except OSError:
            pass
        return best[1] if best else None

    def looks_windows(self, token: str) -> bool:
        token = token.strip('"')
        return bool(DRIVE.match(token)) or token.upper() in ("NUL", "NUL:") or (
            "\\" in token and not token.startswith("\\\\")) or bool(WINVAR.search(token))

    def translate_arg(self, arg: str, cwd: str | None = None) -> str:
        """An argument for a Linux tool: a Windows path in it, alone or
        after `--option=` or `/option:`, becomes the Linux path."""
        for pattern in (r"^(--?[A-Za-z][\w-]*=)(.+)$", r"^(/[A-Za-z][\w-]*:)(.+)$"):
            match = re.match(pattern, arg)
            if match and self.looks_windows(match.group(2)):
                translated = self.win_to_unix(match.group(2), cwd)
                return match.group(1) + (translated or match.group(2))
        if self.looks_windows(arg):
            return self.win_to_unix(arg, cwd) or arg
        return arg

    def leaves_environment(self, unix_path: str) -> str | None:
        """Why a path reached through a drive letter is not inside the
        Windows environment's own drive, or None when it is."""
        inside = os.path.realpath(self.drive_c)
        target = os.path.realpath(unix_path)
        if target == inside:
            return "the whole drive of the Windows environment"
        if target.startswith(inside + "/"):
            return None
        return f"a folder of this computer ({target}) seen from the Windows environment"
