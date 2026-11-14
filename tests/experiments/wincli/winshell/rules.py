# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The safety rules tur-winshell applies before it runs a line.

A Windows command pasted into TUR must not damage the real computer by
being taken literally. The rules (proposed; the report states them):

R1  Disk, partition, boot and firmware commands are refused, always, with
    what they do on Windows and where TUR does that work. Nothing typed in
    a terminal reaches disks or the boot loader.
R2  Power, repair and update commands are not passed on. They are answered
    with the TUR action that has that meaning (shims.py).
R3  A recursive delete or a mirror whose target, reached through a drive
    letter, lies outside the Windows environment's own drive (a mapped
    folder, or another drive letter) is refused unless the person confirms;
    so is one that names the root of a drive.
R4  Registry writes, service and account commands run only inside the
    Windows environment; that is true by construction (they reach Wine's
    registry and services, never the computer's).
R5  A line that starts another shell (cmd /c, powershell -Command, an
    encoded command) is judged by what that shell would run.
R6  No command gets privileges: nothing here calls sudo, pkexec or polkit.

These are lexical rules: a seat belt. A batch file, a variable or a
program can still do what a line does not show, which is why R1 commands
are also shadowed by refusing programs on the Windows PATH, and why the
real wall is the runtime's sandbox, which only shows a Windows program the
folders it was granted.
"""

from __future__ import annotations

import base64
import os
import re
from dataclasses import dataclass

from winpath import WinEnv

DISK = ("works on disks and partitions of a Windows PC. On TUR, disks are managed by TUR Storage, which asks "
        "for confirmation, with the exact device, on a path no command or assistant can operate.")
BOOT = ("changes how a Windows PC starts. TUR's boot loader and its entries are managed by TUR's update and "
        "boot-health services; a command typed here never changes them.")

# R1: head -> what to say.
REFUSE = {
    "format": "'format' erases a volume. It " + DISK,
    "diskpart": "'diskpart' creates, deletes and wipes partitions. It " + DISK,
    "fdisk": "'fdisk' edits the partition table. It " + DISK,
    "clean": None,
    "convert": "'convert' changes a volume's file system. It " + DISK,
    "label": "'label' renames a volume. It " + DISK,
    "mountvol": "'mountvol' mounts and unmounts volumes. It " + DISK,
    "recover": "'recover' reads a damaged disk sector by sector. It " + DISK,
    "sdelete": "'sdelete' overwrites files and free space. It " + DISK,
    "vssadmin": "'vssadmin' manages and deletes restore snapshots. TUR keeps its own snapshots and rollback.",
    "wbadmin": "'wbadmin' manages and deletes Windows backups. TUR keeps its own snapshots and rollback.",
    "manage-bde": "'manage-bde' turns disk encryption on and off. On TUR, encryption is handled by TUR Storage, "
                  "with confirmation on a path no command can operate.",
    "bcdedit": "'bcdedit' edits the boot configuration. It " + BOOT,
    "bcdboot": "'bcdboot' writes boot files. It " + BOOT,
    "bootrec": "'bootrec' rewrites the boot sector and boot records. It " + BOOT,
    "bootsect": "'bootsect' rewrites the boot sector. It " + BOOT,
    "reagentc": "'reagentc' configures the Windows recovery image. It " + BOOT,
}
REFUSE_PS = {
    name: f"'{name}' " + DISK
    for name in ("format-volume", "clear-disk", "initialize-disk", "remove-partition", "new-partition",
                 "set-partition", "resize-partition", "set-disk", "repair-volume", "optimize-volume",
                 "disable-bitlocker", "enable-bitlocker", "clear-bitlockerautounlock")
}
REFUSE_PS.update({
    "stop-computer": "'Stop-Computer' turns a Windows PC off. On TUR use the power menu; a pasted command does "
                     "not turn this computer off.",
    "restart-computer": "'Restart-Computer' restarts a Windows PC. On TUR use the power menu; a pasted command "
                        "does not restart this computer.",
    "set-secureboot": "'Set-SecureBootUEFI' changes firmware variables. " + BOOT,
    "set-securebootuefi": "'Set-SecureBootUEFI' changes firmware variables. " + BOOT,
})
RECURSIVE_CMD = {"del", "erase", "rd", "rmdir"}
RECURSIVE_PS = {"remove-item", "rm", "del", "erase", "rd", "rmdir", "ri"}
WRAPPERS = {"call", "start", "runas", "cmd", "powershell", "pwsh", "forfiles"}


@dataclass
class Verdict:
    action: str  # allow | refuse
    rule: str = ""
    message: str = ""


ALLOW = Verdict("allow")


def head_of(word: str) -> str:
    word = word.strip().strip('"').strip("'").lower()
    word = re.split(r"[\\/]", word)[-1]
    return re.sub(r"\.(exe|com|bat|cmd)$", "", word)


def split_cmd(line: str) -> list[str]:
    """A cmd line as its simple commands: split at unquoted & | ( )."""
    parts, current, quoted, i = [], [], False, 0
    while i < len(line):
        ch = line[i]
        if ch == "^" and i + 1 < len(line) and not quoted:
            current.append(line[i + 1])
            i += 2
            continue
        if ch == '"':
            quoted = not quoted
        if not quoted and ch in "&|()":
            parts.append("".join(current))
            current = []
        else:
            current.append(ch)
        i += 1
    parts.append("".join(current))
    return [part.strip() for part in parts if part.strip()]


def words_of(segment: str) -> list[str]:
    return re.findall(r'"[^"]*"|\S+', segment)


def _strip_control(words: list[str], heads: set[str]) -> list[str]:
    """Drop `@`, and the `if ...` / `for ... do` / `else` in front of the
    command a segment really runs."""
    while words and words[0].startswith("@"):
        words[0] = words[0][1:]
        if not words[0]:
            words = words[1:]
    if not words:
        return words
    first = words[0].lower()
    if first == "do":
        # What is left of `for ... in (set) do command` once the line has
        # been split at the parentheses of the set.
        return _strip_control(words[1:], heads)
    if first == "for":
        lowered = [w.lower() for w in words]
        return _strip_control(words[lowered.index("do") + 1:], heads) if "do" in lowered else []
    if first in ("if", "else"):
        for n, word in enumerate(words[1:], 1):
            if head_of(word) in heads:
                return words[n:]
        return []
    return words


def _recursive_targets(head: str, words: list[str]) -> list[str] | None:
    """Targets of a recursive delete or mirror, or None if it is neither."""
    switches = [w.lower() for w in words[1:] if w.startswith("/")]
    paths = [w for w in words[1:] if not w.startswith("/")]
    if head in RECURSIVE_CMD:
        if not any(re.fullmatch(r"/s", s) for s in switches):
            return None
        return paths or ["."]
    if head == "robocopy":
        if not any(s in ("/mir", "/purge", "/move") for s in switches):
            return None
        return paths[1:2] + (paths[0:1] if "/move" in switches else [])
    return None


def _check_targets(env: WinEnv, cwd: str, command: str, targets: list[str], through_letter_only: bool) -> Verdict:
    if os.environ.get("TUR_WINSHELL_CONFIRM") == "yes":
        return ALLOW
    for target in targets:
        raw = env.expand(target.strip('"').strip("'"))
        if "$" in raw or "%" in raw:
            return Verdict("refuse", "R3", f"'{command}' deletes a folder and everything in it, and its target "
                           f"({target}) cannot be worked out before it runs. It is not run without your "
                           "confirmation.")
        if through_letter_only and raw.startswith("/"):
            continue
        folder = re.sub(r"[\\/]?[^\\/]*[*?][^\\/]*$", "", raw) or "."
        if re.fullmatch(r"[A-Za-z]:[\\/]?", raw.rstrip("*")) or re.fullmatch(r"[A-Za-z]:[\\/]?", folder):
            return Verdict("refuse", "R3", f"'{command}' would delete everything on drive {raw[:2].upper()} of the "
                           "Windows environment. It is not run without your confirmation.")
        unix = env.win_to_unix(folder, cwd)
        if unix is None:
            continue
        why = env.leaves_environment(unix)
        if why:
            return Verdict("refuse", "R3", f"'{command}' deletes a folder and everything in it, and {target} is "
                           f"{why}. A Windows command does not delete this computer's folders without your "
                           "confirmation.")
    return ALLOW


def classify_cmd(line: str, env: WinEnv, cwd: str, depth: int = 0) -> Verdict:
    if depth > 4:
        return ALLOW
    interesting = set(REFUSE) | RECURSIVE_CMD | WRAPPERS | {"robocopy"}
    for segment in split_cmd(line):
        words = _strip_control(words_of(segment), interesting)
        if not words:
            continue
        head = head_of(words[0])
        # R5: what another shell would run.
        if head in ("call", "runas"):
            verdict = classify_cmd(" ".join(words[1:]), env, cwd, depth + 1)
        elif head == "start":
            rest = [w for w in words[1:] if not w.startswith("/") and not (w.startswith('"') and w is words[1])]
            verdict = classify_cmd(" ".join(rest), env, cwd, depth + 1)
        elif head == "cmd":
            verdict = ALLOW
            for n, word in enumerate(words[1:], 1):
                if word.lower() in ("/c", "/k"):
                    inner = " ".join(words[n + 1:])
                    if inner.startswith('"') and inner.endswith('"'):
                        inner = inner[1:-1]
                    verdict = classify_cmd(inner, env, cwd, depth + 1)
                    break
        elif head in ("powershell", "pwsh"):
            verdict = classify_ps(ps_payload(words[1:]), env, cwd, depth + 1)
        elif head in REFUSE and REFUSE[head]:
            verdict = Verdict("refuse", "R1", REFUSE[head])
        elif head == "cipher" and any(w.lower().startswith("/w") for w in words[1:]):
            verdict = Verdict("refuse", "R1", "'cipher /w' overwrites free disk space. It " + DISK)
        else:
            targets = _recursive_targets(head, words)
            verdict = _check_targets(env, cwd, segment, targets, False) if targets else ALLOW
        if verdict.action != "allow":
            return verdict
    return ALLOW


def ps_payload(args: list[str]) -> str:
    """The script a powershell command line would run."""
    n = 0
    while n < len(args):
        low = args[n].lower()
        if low in ("-command", "-c"):
            rest = " ".join(args[n + 1:])
            return rest[1:-1] if len(rest) > 1 and rest[0] == '"' and rest[-1] == '"' else rest
        if low in ("-encodedcommand", "-enc", "-ec", "-e") and n + 1 < len(args):
            try:
                return base64.b64decode(args[n + 1]).decode("utf-16-le")
            except (ValueError, UnicodeDecodeError):
                return ""
        if low in ("-file", "-f"):
            return ""
        if low in ("-executionpolicy", "-ep", "-windowstyle", "-w", "-workingdirectory", "-wd"):
            n += 1
        elif not low.startswith("-"):
            rest = " ".join(args[n:])
            return rest[1:-1] if len(rest) > 1 and rest[0] == '"' and rest[-1] == '"' else rest
        n += 1
    return ""


def classify_ps(script: str, env: WinEnv, cwd: str, depth: int = 0) -> Verdict:
    if depth > 4 or not script:
        return ALLOW
    for segment in re.split(r"[;|\n{}()]+", script):
        words = re.findall(r"\"[^\"]*\"|'[^']*'|\S+", segment)
        while words and words[0] in ("&", "."):
            words = words[1:]
        if not words:
            continue
        head = head_of(words[0])
        if head in REFUSE_PS:
            return Verdict("refuse", "R1" if "-" in head and "computer" not in head else "R2", REFUSE_PS[head])
        if head in REFUSE and REFUSE[head]:
            return Verdict("refuse", "R1", REFUSE[head])
        if head == "cmd":
            for n, word in enumerate(words[1:], 1):
                if word.lower() in ("/c", "/k"):
                    inner = " ".join(words[n + 1:]).strip("\"'")
                    verdict = classify_cmd(inner, env, cwd, depth + 1)
                    if verdict.action != "allow":
                        return verdict
                    break
        if head in ("powershell", "pwsh"):
            verdict = classify_ps(ps_payload(words[1:]), env, cwd, depth + 1)
            if verdict.action != "allow":
                return verdict
        if head in RECURSIVE_PS and any(re.fullmatch(r"-r(e(c(u(r(s(e)?)?)?)?)?)?", w.lower()) for w in words[1:]):
            targets, skip = [], False
            for word in words[1:]:
                if skip:
                    skip = False
                    continue
                low = word.lower()
                if low in ("-erroraction", "-ea", "-filter", "-include", "-exclude"):
                    skip = True
                elif low.startswith("-"):
                    continue
                else:
                    targets.append(word)
            verdict = _check_targets(env, cwd, segment.strip(), targets or ["."], True)
            if verdict.action != "allow":
                return verdict
    return ALLOW
