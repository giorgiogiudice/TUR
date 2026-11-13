#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Generate PaxCommands.qml from commands.json, and check the keys.

    gen_commands.py --write    regenerate PaxCommands.qml
    gen_commands.py --check    fail if PaxCommands.qml is stale, if commands.json
                               breaks a rule, or if a file in the repository
                               binds one of P@x's commands to other keys (the
                               default)

commands.json is the one source of P@x's keyboard commands: what P@x's
indicator shows (the owner's answer of 12 November 2026,
desktop-experience.md 17.1, answer 2) and what the programs that bind the keys
bind. The generated file is committed, as the token files are, and the
`boundary` job runs --check.

The bindings. A group of a desktop file under desktop/ (the entry, or one of
its actions) that has an X-KDE-Shortcuts line binds the command named by the
same group's X-TUR-Pax-Command line, and must bind it to the keys
commands.json gives. In a desktop file of one of P@x's programs (org.tur.Pax*)
every group with a key must name its command. tur-pax-ui's desktop file
(desktop/apps/tur-pax-ui/data/org.tur.PaxUi.desktop) binds three this way.
The check fails if a command marked bound has no group that binds it. Needs
only the Python standard library.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[2]
SOURCE = HERE / "commands.json"
OUTPUT = HERE / "PaxCommands.qml"

HEADER = """\
// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// GENERATED from pax/commands.json by pax/gen_commands.py. Do not edit.
// Change commands.json, then run: python3 desktop/kit/pax/gen_commands.py --write
"""

KEYS = re.compile(r"^(Hold )?((Meta|Ctrl|Alt|Shift)\+)+[A-Za-z0-9/]+$|^(Hold )?((Meta|Ctrl|Alt|Shift)\+)+(Esc|Space|Tab|Return)$")
REQUIRED = ("stop", "keys")


def load() -> dict:
    return json.loads(SOURCE.read_text(encoding="utf-8"))


def validate(data: dict) -> list[str]:
    errors = []
    seen_ids: set[str] = set()
    seen_keys: dict[str, str] = {}
    for command in data["commands"]:
        cid = command.get("id", "")
        if not re.fullmatch(r"[a-z][a-z-]*", cid):
            errors.append(f"command {cid!r}: an id is lower-case words")
        if cid in seen_ids:
            errors.append(f"command {cid}: listed twice")
        seen_ids.add(cid)
        keys = command.get("keys", "")
        if not KEYS.match(keys):
            errors.append(f"command {cid}: {keys!r} is not a key combination such as Meta+Esc")
        if keys in seen_keys:
            errors.append(f"command {cid}: {keys} is already {seen_keys[keys]}")
        seen_keys[keys] = cid
        for field in ("action", "boundBy"):
            if not command.get(field):
                errors.append(f"command {cid}: no {field}")
        for field in ("bound", "verified"):
            if not isinstance(command.get(field), bool):
                errors.append(f"command {cid}: {field} is true or false")
        if '"' in command.get("action", "") or "\\" in command.get("action", ""):
            errors.append(f"command {cid}: the action has a quote or a backslash")
    for cid in REQUIRED:
        if cid not in seen_ids:
            errors.append(f"there is no {cid!r} command: the indicator shows it at all times")
    return errors


def render(data: dict) -> str:
    by_id = {command["id"]: command for command in data["commands"]}
    out = [HEADER.rstrip("\n"), "pragma Singleton", "", "import QtQuick", "",
           "// P@x's keyboard commands. Only P@x's surfaces import Tur.Kit.Pax.",
           "QtObject {",
           "    // Shown by P@x's indicator at all times while P@x is on.",
           f"    readonly property string stopKeys: \"{by_id['stop']['keys']}\"",
           "    // The command that shows the full list.",
           f"    readonly property string listKeys: \"{by_id['keys']['keys']}\"",
           "",
           "    // Every command, in the order of commands.json.",
           "    readonly property var commands: ["]
    rows = []
    for command in data["commands"]:
        rows.append("        {{ id: \"{id}\", keys: \"{keys}\", action: qsTranslate(\"PaxCommands\", \"{action}\"), "
                    "bound: {bound} }}".format(id=command["id"], keys=command["keys"], action=command["action"],
                                               bound="true" if command["bound"] else "false"))
    out.append(",\n".join(rows))
    out += ["    ]", "}"]
    return "\n".join(out) + "\n"


def groups(text: str) -> list[tuple[str, dict[str, str]]]:
    """The groups of a desktop file, each with its keys: [("Desktop Entry", {...}), ...]."""
    out: list[tuple[str, dict[str, str]]] = []
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        header = re.fullmatch(r"\[(.+)\]", line)
        if header:
            out.append((header.group(1), {}))
        elif "=" in line and out:
            key, value = line.split("=", 1)
            out[-1][1].setdefault(key.strip(), value.strip())
    return out


def bindings(data: dict, root: pathlib.Path = ROOT) -> tuple[list[str], list[str]]:
    """Files that bind one of P@x's commands, and where they disagree."""
    keys = {command["id"]: command["keys"] for command in data["commands"]}
    found, errors = [], []
    bound_ids = set()
    for path in sorted((root / "desktop").rglob("*.desktop")):
        if "build" in path.relative_to(root / "desktop").parts:
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        relative = path.relative_to(root).as_posix()
        for name, entries in groups(text):
            shortcut = entries.get("X-KDE-Shortcuts")
            if shortcut is None:
                continue
            command = entries.get("X-TUR-Pax-Command")
            if not path.name.startswith("org.tur.Pax") and command is None:
                continue
            where = f"{relative} [{name}]"
            found.append(where)
            if command is None:
                errors.append(f"{where}: binds {shortcut} but names no X-TUR-Pax-Command")
                continue
            bound_ids.add(command)
            if command not in keys:
                errors.append(f"{where}: X-TUR-Pax-Command={command} is not in commands.json")
            elif shortcut != keys[command]:
                errors.append(f"{where}: binds {command} to {shortcut}; commands.json says {keys[command]}")
    for command in data["commands"]:
        if command["bound"] and command["id"] not in bound_ids:
            errors.append(f"command {command['id']} is marked bound and no file binds it")
    return found, errors


def self_test(data: dict) -> int:
    """The check of the bindings catches a file that disagrees, and passes one that agrees."""
    import tempfile
    cases = {
        "agrees": ("X-KDE-Shortcuts=Meta+Space\nX-TUR-Pax-Command=ask\n", None),
        "other keys": ("X-KDE-Shortcuts=Meta+Shift+Space\nX-TUR-Pax-Command=ask\n", "commands.json says Meta+Space"),
        "no command": ("X-KDE-Shortcuts=Meta+Space\n", "names no X-TUR-Pax-Command"),
        "unknown": ("X-KDE-Shortcuts=Meta+Q\nX-TUR-Pax-Command=quit\n", "is not in commands.json"),
        "action agrees": ("X-KDE-Shortcuts=Meta+Space\nX-TUR-Pax-Command=ask\n\n[Desktop Action stop]\n"
                          "X-KDE-Shortcuts=Meta+Esc\nX-TUR-Pax-Command=stop\n", None),
        "action other keys": ("X-KDE-Shortcuts=Meta+Space\nX-TUR-Pax-Command=ask\n\n[Desktop Action stop]\n"
                              "X-KDE-Shortcuts=Ctrl+Esc\nX-TUR-Pax-Command=stop\n", "commands.json says Meta+Esc"),
        "action no command": ("X-TUR-Pax-Command=ask\nX-KDE-Shortcuts=Meta+Space\n\n[Desktop Action keys]\n"
                              "X-KDE-Shortcuts=Meta+/\n", "names no X-TUR-Pax-Command"),
    }
    failures = 0
    for name, (body, expected) in cases.items():
        with tempfile.TemporaryDirectory() as holder:
            root = pathlib.Path(holder)
            (root / "desktop" / "apps").mkdir(parents=True)
            (root / "desktop" / "apps" / "org.tur.PaxUi.desktop").write_text("[Desktop Entry]\n" + body)
            _, problems = bindings(data, root)
        ok = (not problems) if expected is None else any(expected in problem for problem in problems)
        if not ok:
            failures += 1
            print(f"self-test {name}: expected {expected!r}, got {problems}", file=sys.stderr)
    broken = json.loads(json.dumps(data))
    broken["commands"] = [c for c in broken["commands"] if c["id"] != "stop"]
    if not any("no 'stop' command" in e for e in validate(broken)):
        failures += 1
        print("self-test: a list without the stop command was not refused", file=sys.stderr)
    print(f"gen_commands self-test: {len(cases) + 1} cases, {failures} failed")
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    data = load()
    if args.self_test:
        return self_test(data)
    errors = validate(data)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    text = render(data)
    if args.write:
        OUTPUT.write_text(text, encoding="utf-8")
        print(f"wrote {OUTPUT.relative_to(ROOT)}")
        return 0
    status = 0
    if not OUTPUT.exists() or OUTPUT.read_text(encoding="utf-8") != text:
        print(f"::error::{OUTPUT.relative_to(ROOT)} is stale: run desktop/kit/pax/gen_commands.py --write",
              file=sys.stderr)
        status = 1
    found, problems = bindings(data)
    for problem in problems:
        print(f"::error::{problem}", file=sys.stderr)
        status = 1
    unbound = [c["id"] for c in data["commands"] if not c["bound"]]
    print(f"P@x's commands: {len(data['commands'])}; files that bind them: {len(found)}"
          + (f" ({', '.join(found)})" if found else "")
          + (f"; not bound by anything yet: {', '.join(unbound)}" if unbound else ""))
    return status


if __name__ == "__main__":
    sys.exit(main())
