#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Wine's registry files: read, write, compare, and replay changes on another
registry (the three-way "registry rebase" of the layers experiment).

Written from Wine 11.20's server/registry.c and server/unicode.c (LGPL; read,
not copied). What that source says, and what this module relies on:

* A prefix has three registry files: system.reg (HKEY_LOCAL_MACHINE),
  user.reg (HKEY_CURRENT_USER) and userdef.reg (HKEY_USERS\\.Default)
  (`init_registry`). Each is text: the line `WINE REGISTRY Version 2`, a
  comment naming the branch, `#arch=win64`, then one block per key.
* A key block is `[Path\\\\With\\\\Escapes] <seconds since 1970>`, then
  `#time=<hex, 100 ns since 1601>`, optionally `#class="..."` and `#link`,
  then one line per value (`save_subkeys`). `load_key` takes the seconds from
  the bracket line; `#time=` then sets the exact time, and only on a key that
  has none yet (`update_key_time`).
* A key is written only when it has values, or no subkeys, or a class, or is
  a symbolic link: a key that only holds subkeys exists through them
  (`save_subkeys`). Volatile keys are never written.
* Values: `"name"=` or `@=`, then `"string"`, `str(2):"..."`, `str(7):"..."`,
  `dword:%08x`, `hex:..` or `hex(type):..`, hex wrapped with a backslash at
  the end of the line (`dump_value`, `load_value`).
* Names are compared without regard to case, by 16-bit unit, shorter first
  when one is a prefix of the other (`find_subkey`, `find_value`,
  `memicmp_strW`); subkeys and values are kept in that order, so that is the
  order of the file.
* Strings are written in ASCII: a backslash before a backslash and before the
  two delimiters, `\\x%04x` or `\\x%x` for a unit above 127, C or octal
  escapes below 32 (`dump_strW`); `parse_strW` reads those and UTF-8.
* A branch is saved only when something in it changed (`save_branch`:
  `KEY_DIRTY`), into a temporary file `reg<pid><n>.tmp` in the prefix that is
  then renamed over the old file; when the old file has more than one link or
  is not a regular file it is truncated and written in place. Saves happen
  every 30 seconds (`save_period`) and when the server exits.

Values are kept as the text Wine wrote, and compared as text: every file here
was written by the same `dump_value`. Key and value names are decoded for
comparison only.

    winereg.py --self-test
    winereg.py roundtrip FILE          parse and write back; say if it differs
    winereg.py stats FILE
    winereg.py diff OLD NEW DELTA.json
    winereg.py apply BASE DELTA.json OUT [--noise NOISE.json]
    winereg.py rebase OLD-BASE MINE NEW-BASE OUT [--noise NOISE.json]
"""

from __future__ import annotations

import json
import os
import pathlib
import sys
import time

MAGIC = "WINE REGISTRY Version 2"
TICKS_1601_TO_1970 = 86400 * (369 * 365 + 89) * 10_000_000

_SIMPLE = {"a": 7, "b": 8, "e": 27, "f": 12, "n": 10, "r": 13, "t": 9, "v": 11}
_HEX = "0123456789abcdefABCDEF"


def unescape(text: str) -> list[int]:
    """`parse_strW`: an escaped string as 16-bit units."""
    units: list[int] = []
    at, end = 0, len(text)
    while at < end:
        ch = text[at]
        if ch == "\\" and at + 1 < end:
            nxt = text[at + 1]
            if nxt in _SIMPLE:
                units.append(_SIMPLE[nxt])
                at += 2
                continue
            if nxt == "x":
                at += 2
                digits = ""
                while at < end and len(digits) < 4 and text[at] in _HEX:
                    digits += text[at]
                    at += 1
                units.append(int(digits, 16) if digits else ord("x"))
                continue
            if nxt in "01234567":
                at += 1
                digits = ""
                while at < end and len(digits) < 3 and text[at] in "01234567":
                    digits += text[at]
                    at += 1
                units.append(int(digits, 8) & 0xFFFF)
                continue
            # An unrecognised escape is the character itself.
            at += 1
            ch = text[at]
        code = ord(ch)
        if code > 0xFFFF:
            code -= 0x10000
            units += [0xD800 | (code >> 10), 0xDC00 | (code & 0x3FF)]
        else:
            units.append(code)
        at += 1
    return units


def _lower(unit: int) -> int:
    if unit < 0x41 or 0xD800 <= unit <= 0xDFFF:
        return unit
    if unit < 0x80:
        return unit + 32 if unit <= 0x5A else unit
    lowered = chr(unit).lower()
    if len(lowered) == 1 and ord(lowered) <= 0xFFFF:
        return ord(lowered)
    return unit


def fold(units: list[int]) -> tuple[int, ...]:
    """The comparison key of a name: Wine's order is this tuple's order."""
    return tuple(_lower(unit) for unit in units)


def key_id(raw_name: str) -> tuple[tuple[int, ...], ...]:
    """A key path as its folded components."""
    parts: list[tuple[int, ...]] = []
    current: list[int] = []
    for unit in unescape(raw_name):
        if unit == 0x5C:
            parts.append(fold(current))
            current = []
        else:
            current.append(unit)
    if current or parts:
        parts.append(fold(current))
    return tuple(parts)


def _closing(text: str, start: int, closer: str) -> int:
    """The index of the first unescaped `closer` at or after `start`."""
    at = start
    while at < len(text):
        if text[at] == "\\":
            at += 2
            continue
        if text[at] == closer:
            return at
        at += 1
    return -1


def value_name(raw_line: str) -> tuple[str, int] | None:
    """The raw name of a value line (`@` or the quoted text without its
    quotes) and the index just after it."""
    if raw_line.startswith("@"):
        return "", 1
    if raw_line.startswith('"'):
        close = _closing(raw_line, 1, '"')
        if close == -1:
            return None
        return raw_line[1:close], close + 1
    return None


class Key:
    """One key as the file has it."""

    __slots__ = ("name", "ident", "seconds", "time", "klass", "link", "values")

    def __init__(self, name: str, seconds: int | None = None) -> None:
        self.name = name                     # the escaped path, as written
        self.ident = key_id(name)
        self.seconds = seconds               # from the bracket line
        self.time: str | None = None         # the `#time=` line's value
        self.klass: str | None = None        # the `#class=` line's value, raw
        self.link = False
        # folded value name -> the whole raw value text (may span lines)
        self.values: dict[tuple[int, ...], str] = {}

    def copy(self) -> "Key":
        other = Key.__new__(Key)
        other.name, other.ident, other.seconds = self.name, self.ident, self.seconds
        other.time, other.klass, other.link = self.time, self.klass, self.link
        other.values = dict(self.values)
        return other

    def stamp(self) -> list:
        return [self.seconds, self.time]

    def content(self) -> tuple:
        return (self.klass, self.link, tuple(sorted(self.values.items())))


def stamp_now() -> list:
    now = time.time()
    ticks = int(now * 10_000_000) + TICKS_1601_TO_1970
    return [int(now), f"{ticks >> 32:x}{ticks & 0xFFFFFFFF:08x}"]


class Registry:
    """One registry file."""

    def __init__(self) -> None:
        self.preamble: list[str] = [MAGIC]
        self.keys: dict[tuple, Key] = {}
        self.warnings: list[str] = []

    # ------------------------------------------------------------- reading

    @classmethod
    def parse(cls, text: str) -> "Registry":
        registry = cls()
        lines = text.split("\n")
        if not lines or lines[0].rstrip("\r") != MAGIC:
            raise ValueError("not a Wine registry file (no 'WINE REGISTRY Version 2' line)")
        registry.preamble = []
        key: Key | None = None
        at, count = 1, len(lines)        # the first line was checked above
        while at < count:
            raw = lines[at].rstrip("\r")
            at += 1
            line = raw.lstrip(" \t")
            if not line:
                continue
            first = line[0]
            if first == "[":
                close = _closing(line, 1, "]")
                if close == -1:
                    registry.warnings.append(f"line {at}: malformed key")
                    key = None
                    continue
                rest = line[close + 1:].split()
                seconds = int(rest[0]) if rest and rest[0].isdigit() else None
                new = Key(line[1:close], seconds)
                existing = registry.keys.get(new.ident)
                if existing is None:
                    registry.keys[new.ident] = new
                    key = new
                else:
                    key = existing       # a key named twice: values add up, as in Wine
            elif first in "@\"":
                if key is None:
                    registry.warnings.append(f"line {at}: value without key")
                    continue
                named = value_name(line)
                if named is None:
                    registry.warnings.append(f"line {at}: malformed value name")
                    continue
                text_lines = [line]
                # Only hex data is wrapped: a string ends with its quote.
                while text_lines[-1].endswith("\\") and at < count and \
                        line[named[1]:].lstrip(" \t=").startswith("hex"):
                    text_lines.append(lines[at].rstrip("\r"))
                    at += 1
                key.values[fold(unescape(named[0]))] = "\n".join(text_lines)
            elif first == "#":
                if key is None:
                    registry.preamble.append(raw)
                elif line.startswith("#time="):
                    if key.time is None:
                        key.time = line[6:]
                elif line.startswith("#class="):
                    key.klass = line[7:]
                elif line.startswith("#link"):
                    key.link = True
            elif first == ";":
                if key is None:
                    registry.preamble.append(raw)
            else:
                registry.warnings.append(f"line {at}: unrecognised input")
        return registry

    @classmethod
    def load(cls, path: os.PathLike | str) -> "Registry":
        data = pathlib.Path(path).read_bytes()
        return cls.parse(data.decode("utf-8", errors="surrogateescape"))

    # ------------------------------------------------------------- writing

    def existing(self) -> set[tuple]:
        """Every key that exists: the written ones and their ancestors."""
        found: set[tuple] = set()
        for ident in self.keys:
            while ident and ident not in found:
                found.add(ident)
                ident = ident[:-1]
        if () in self.keys:
            found.add(())
        return found

    def parents(self) -> set[tuple]:
        """Every key that has a subkey."""
        found: set[tuple] = set()
        for ident in self.keys:
            ident = ident[:-1]
            while ident and ident not in found:
                found.add(ident)
                ident = ident[:-1]
        return found

    def dump(self) -> str:
        """The file as Wine's `save_all_subkeys` writes it."""
        out = [MAGIC]
        comments = [line for line in self.preamble if line.startswith(";")]
        options = [line for line in self.preamble if line.startswith("#")]
        out += comments
        for option in options:
            out += ["", option]
        parents = self.parents()
        for ident in sorted(self.keys):
            key = self.keys[ident]
            if not key.values and key.klass is None and not key.link and ident in parents:
                continue                 # exists through its subkeys
            seconds, stamp = key.seconds, key.time
            if seconds is None or stamp is None:
                seconds, stamp = stamp_now() if seconds is None else (seconds, _ticks_of(seconds))
            out += ["", f"[{key.name}] {seconds}", f"#time={stamp}"]
            if key.klass is not None:
                out.append(f"#class={key.klass}")
            if key.link:
                out.append("#link")
            out += [key.values[name] for name in sorted(key.values)]
        return "\n".join(out) + "\n"

    def save(self, path: os.PathLike | str) -> None:
        """Through a temporary file and a rename, as Wine does."""
        path = pathlib.Path(path)
        temporary = path.with_name(f".{path.name}.tur-rebase.tmp")
        temporary.write_bytes(self.dump().encode("utf-8", errors="surrogateescape"))
        os.replace(temporary, path)

    def copy(self) -> "Registry":
        other = Registry()
        other.preamble = list(self.preamble)
        other.keys = {ident: key.copy() for ident, key in self.keys.items()}
        return other

    def stats(self) -> dict:
        return {"keys": len(self.keys), "values": sum(len(key.values) for key in self.keys.values()),
                "warnings": len(self.warnings)}


def _ticks_of(seconds: int) -> str:
    ticks = seconds * 10_000_000 + TICKS_1601_TO_1970
    return f"{ticks >> 32:x}{ticks & 0xFFFFFFFF:08x}"


def raw_value_name(text: str) -> str:
    named = value_name(text)
    return named[0] if named else ""


# ------------------------------------------------------------------ the delta
#
# A delta is what one registry changed relative to another, with what was
# there before, so that replaying it on a third registry can tell "the other
# side did not touch this" from "both changed it".
#
#   {"format": 1, "branch": "...", "ops": [
#     {"key": "<escaped path>", "op": "delete", "old": {<escaped path>: <digest>}},
#     {"key": "<escaped path>", "op": "change", "created": true|false,
#      "stamp": [seconds, "#time"], "class": [old, new] (when it changed),
#      "link": [old, new] (when it changed),
#      "set": [[<raw value text>, <old raw value text or null>], ...],
#      "del": [[<raw value name>, <old raw value text>], ...]}]}


def _digest(key: Key) -> str:
    import hashlib
    blob = repr(key.content()).encode("utf-8", errors="surrogateescape")
    return hashlib.sha256(blob).hexdigest()[:16]


def diff(old: Registry, new: Registry) -> dict:
    """What `new` changed relative to `old`. Times alone are not a change."""
    ops: list[dict] = []
    old_existing, new_existing = old.existing(), new.existing()

    # Keys that are gone, as the topmost deleted key of each deleted subtree.
    gone = sorted(ident for ident in old_existing if ident not in new_existing)
    tops = [ident for ident in gone if ident[:-1] in new_existing or len(ident) == 1]
    by_top: dict[tuple, dict[str, str]] = {top: {} for top in tops}
    names: dict[tuple, str] = {}
    for ident in sorted(old.keys):
        if ident in new_existing:
            continue
        key = old.keys[ident]
        for depth in range(1, len(ident) + 1):
            if ident[:depth] in by_top:
                by_top[ident[:depth]][key.name] = _digest(key)
                names.setdefault(ident[:depth], _prefix_name(key.name, depth))
                break
    for top in tops:
        if top in names:
            ops.append({"key": names[top], "op": "delete", "old": by_top[top]})

    for ident in sorted(new.keys):
        mine = new.keys[ident]
        theirs = old.keys.get(ident)
        created = ident not in old_existing
        op: dict = {"key": mine.name, "op": "change", "created": created, "stamp": mine.stamp(),
                    "set": [], "del": []}
        before = theirs.values if theirs else {}
        for name, text in mine.values.items():
            if before.get(name) != text:
                op["set"].append([text, before.get(name)])
        for name, text in before.items():
            if name not in mine.values:
                op["del"].append([raw_value_name(text), text])
        old_class = theirs.klass if theirs else None
        if mine.klass != old_class:
            op["class"] = [old_class, mine.klass]
        old_link = theirs.link if theirs else False
        if mine.link != old_link:
            op["link"] = [old_link, mine.link]
        if op["set"] or op["del"] or "class" in op or "link" in op or created:
            ops.append(op)
    # Keys the old file wrote that the new one only has through subkeys: their
    # values were deleted.
    for ident in sorted(old.keys):
        if ident in new.keys or ident not in new_existing:
            continue
        theirs = old.keys[ident]
        if theirs.values:
            ops.append({"key": theirs.name, "op": "change", "created": False, "stamp": theirs.stamp(),
                        "set": [], "del": [[raw_value_name(text), text] for text in theirs.values.values()]})
    return {"format": 1, "ops": ops}


def _prefix_name(raw_name: str, depth: int) -> str:
    """The first `depth` components of an escaped path, still escaped."""
    at, seen = 0, 0
    while at < len(raw_name):
        if raw_name[at] == "\\" and at + 1 < len(raw_name):
            if raw_name[at + 1] == "\\":
                seen += 1
                if seen == depth:
                    return raw_name[:at]
            at += 2
            continue
        at += 1
    return raw_name


def delta_size(delta: dict) -> dict:
    sets = sum(len(op.get("set", [])) for op in delta["ops"])
    dels = sum(len(op.get("del", [])) for op in delta["ops"])
    return {"keys_changed": sum(1 for op in delta["ops"] if op["op"] == "change" and not op.get("created")),
            "keys_created": sum(1 for op in delta["ops"] if op["op"] == "change" and op.get("created")),
            "keys_deleted": sum(1 for op in delta["ops"] if op["op"] == "delete"),
            "values_set": sets, "values_deleted": dels}


def touched(delta: dict) -> set:
    """Every (key, value) a delta touches, for use as a noise filter: a pair
    of folded key path and folded value name, and (key, None) for a deleted
    key."""
    found: set = set()
    for op in delta["ops"]:
        ident = key_id(op["key"])
        if op["op"] == "delete":
            found.add((ident, None))
            continue
        for text, _old in op["set"]:
            found.add((ident, fold(unescape(raw_value_name(text)))))
        for name, _old in op["del"]:
            found.add((ident, fold(unescape(name))))
    return found


def apply(base: Registry, delta: dict, noise: set | None = None, prefer: str = "mine") -> tuple[Registry, dict]:
    """Replays a delta on `base`. Returns the result and a report:
    what was applied, what was left out as Wine's own noise, and every
    conflict (both sides changed the same thing differently) with how it was
    settled. `prefer` is "mine" (the delta wins a conflict) or "base"."""
    result = base.copy()
    noise = noise or set()
    report = {"applied": 0, "already": 0, "noise_skipped": 0, "conflicts": []}
    existing = result.existing()

    def conflict(kind: str, key: str, value: str | None, mine, theirs) -> bool:
        """Records a conflict; true when the delta's side is to be taken."""
        report["conflicts"].append({"kind": kind, "key": key, "value": value, "mine": _short(mine),
                                    "base": _short(theirs), "taken": prefer})
        return prefer == "mine"

    for op in delta["ops"]:
        ident = key_id(op["key"])
        if op["op"] == "delete":
            if (ident, None) in noise:
                report["noise_skipped"] += 1
                continue
            inside = [other for other in result.keys if other[:len(ident)] == ident]
            if not inside and ident not in existing:
                report["already"] += 1
                continue
            now = {result.keys[other].name: _digest(result.keys[other]) for other in inside}
            folded_old = {key_id(name): digest for name, digest in op["old"].items()}
            folded_now = {key_id(name): digest for name, digest in now.items()}
            take = True
            if folded_now != folded_old:
                changed = sorted(name for name in now
                                 if folded_old.get(key_id(name)) != now[name])
                take = conflict("delete-vs-change", op["key"], None, "key deleted",
                                f"{len(changed)} key(s) under it differ from what was deleted: {changed[:3]}")
            if take:
                for other in inside:
                    del result.keys[other]
                parent = ident[:-1]
                if parent and parent not in result.existing():
                    # The parent existed through this key only; it stays, empty.
                    stub = Key(_prefix_name(op["key"], len(parent)))
                    stub.seconds, stub.time = stamp_now()
                    result.keys[parent] = stub
                existing = result.existing()
                report["applied"] += 1
            continue

        key = result.keys.get(ident)
        key_was_there = ident in existing
        if key is None:
            if not key_was_there and not op.get("created"):
                # The delta changed a key that the new base no longer has.
                wanted = [text for text, _old in op["set"]]
                if not wanted and "class" not in op and "link" not in op:
                    report["already"] += 1
                    continue
                if not conflict("change-vs-delete", op["key"], None, f"{len(wanted)} value(s) set", "key deleted"):
                    continue
            key = Key(op["key"])
            key.seconds, key.time = op.get("stamp") or stamp_now()
            result.keys[ident] = key
            existing.update(ident[:depth] for depth in range(1, len(ident) + 1))
        changed_here = False
        for text, old in op["set"]:
            name = fold(unescape(raw_value_name(text)))
            if (ident, name) in noise:
                report["noise_skipped"] += 1
                continue
            now = key.values.get(name)
            if now == text:
                report["already"] += 1
                continue
            if now == old or (old is None and now is None):
                take = True
            elif old is None:
                take = conflict("add-vs-add", op["key"], raw_value_name(text), text, now)
            elif now is None:
                take = conflict("change-vs-delete", op["key"], raw_value_name(text), text, None)
            else:
                take = conflict("change-vs-change", op["key"], raw_value_name(text), text, now)
            if take:
                key.values[name] = text
                changed_here = True
                report["applied"] += 1
        for raw_name, old in op["del"]:
            name = fold(unescape(raw_name))
            if (ident, name) in noise:
                report["noise_skipped"] += 1
                continue
            now = key.values.get(name)
            if now is None:
                report["already"] += 1
                continue
            if now == old or conflict("delete-vs-change", op["key"], raw_name, None, now):
                del key.values[name]
                changed_here = True
                report["applied"] += 1
        if "class" in op and key.klass != op["class"][1]:
            if key.klass == op["class"][0] or conflict("class", op["key"], None, op["class"][1], key.klass):
                key.klass = op["class"][1]
                changed_here = True
        if "link" in op and key.link != op["link"][1]:
            key.link = op["link"][1]
            changed_here = True
        if op.get("created") and not key_was_there:
            changed_here = True
            report["applied"] += 1
        if changed_here and op.get("stamp"):
            key.seconds, key.time = op["stamp"]
    return result, report


def _short(text) -> str | None:
    if text is None:
        return None
    text = str(text)
    return text if len(text) <= 300 else text[:300] + f"... ({len(text)} characters)"


def rebase(old_base: Registry, mine: Registry, new_base: Registry, noise: set | None = None,
           prefer: str = "mine") -> tuple[Registry, dict, dict]:
    """Three-way: what `mine` changed relative to `old_base`, replayed on
    `new_base`. Returns the result, the delta and the report."""
    delta = diff(old_base, mine)
    result, report = apply(new_base, delta, noise, prefer)
    return result, delta, report


def same(left: Registry, right: Registry) -> bool:
    """Equal in everything but times and the order of the file."""
    if left.existing() != right.existing():
        return False
    for ident in set(left.keys) | set(right.keys):
        one, two = left.keys.get(ident), right.keys.get(ident)
        if (one.content() if one else (None, False, ())) != (two.content() if two else (None, False, ())):
            return False
    return True


# ------------------------------------------------------------------ self-test

_SAMPLE_BASE = "\n".join([
    MAGIC, ";; All keys relative to \\\\Machine", "", "#arch=win64", "",
    "[Software\\\\Classes\\\\.txt] 1700000000", "#time=1da0f0000000000", "@=\"txtfile\"",
    "\"Content Type\"=\"text/plain\"", "",
    "[Software\\\\Microsoft\\\\Windows NT\\\\CurrentVersion] 1700000000", "#time=1da0f0000000000",
    "\"CurrentBuild\"=\"19045\"",
    "\"DigitalProductId\"=hex:a4,00,00,00,03,00,00,00,35,35,30,34,31,2d,30,30,31,36,\\",
    "  38,2d,30,32,36,2d,36,35,36,37,37,30,2d,37,35,00,00,00",
    "\"ProductName\"=\"Windows 10 Pro\"", "",
    "[Software\\\\Wine\\\\Empty] 1700000000", "#time=1da0f0000000000", "",
    "[Software\\\\Wine\\\\Gone\\\\Deeper] 1700000000", "#time=1da0f0000000000", "\"x\"=dword:00000001", "",
    "[System\\\\CurrentControlSet] 1700000000", "#time=1da0f0000000000", "#link",
    "\"SymbolicLinkValue\"=hex(6):5c,00,52,00", "",
    "[System\\\\Odd\\\\Name with \\[brackets\\] and \\x00e9] 1700000000", "#time=1da0f0000000000",
    "#class=\"cls\"", "@=\"\"", "\"quoted \\\"name\\\"\"=str(2):\"%SystemRoot%\\\\x\"", ""])


def self_test() -> int:
    failures: list[str] = []

    def check(name: str, condition: bool) -> None:
        print(("ok    " if condition else "FAIL  ") + name)
        if not condition:
            failures.append(name)

    base = Registry.parse(_SAMPLE_BASE)
    check("parses without warnings", not base.warnings and len(base.keys) == 6)
    check("round trip is byte for byte", base.dump() == _SAMPLE_BASE)
    check("unescape reads hex, octal and C escapes",
          unescape("a\\x00e9\\n\\1\\\\[") == [97, 0xE9, 10, 1, 0x5C, ord("[")])
    check("key identity ignores case", key_id("Software\\\\WINE") == key_id("software\\\\wine"))
    check("a wrapped hex value is one value",
          base.keys[key_id("Software\\\\Microsoft\\\\Windows NT\\\\CurrentVersion")]
          .values[fold(unescape("DigitalProductId"))].count("\n") == 1)
    check("order: parent first, shorter name first",
          sorted([key_id("A\\\\Bc"), key_id("A\\\\B"), key_id("A"), key_id("a\\\\b\\\\c")])
          == [key_id("A"), key_id("A\\\\B"), key_id("a\\\\b\\\\c"), key_id("A\\\\Bc")])

    # The application: adds a key, changes a value, deletes a value, deletes a
    # key with its subtree, gives an empty key a subkey.
    mine = base.copy()
    version = mine.keys[key_id("Software\\\\Microsoft\\\\Windows NT\\\\CurrentVersion")]
    version.values[fold(unescape("ProductName"))] = "\"ProductName\"=\"Changed By App\""
    del mine.keys[key_id("Software\\\\Classes\\\\.txt")].values[fold(unescape("Content Type"))]
    del mine.keys[key_id("Software\\\\Wine\\\\Gone\\\\Deeper")]
    added = Key("Software\\\\App\\\\Settings", 1700000100)
    added.time = "1da0f0000000100"
    added.values[fold(unescape("Path"))] = "\"Path\"=\"C:\\\\App\""
    mine.keys[added.ident] = added
    child = Key("Software\\\\Wine\\\\Empty\\\\Child", 1700000100)
    child.time = "1da0f0000000100"
    mine.keys[child.ident] = child
    mine = Registry.parse(mine.dump())           # as Wine would have saved it
    check("an empty key with a subkey is not written", key_id("Software\\\\Wine\\\\Empty") not in mine.keys)

    delta = diff(base, mine)
    size = delta_size(delta)
    check("delta: one key deleted", size["keys_deleted"] == 1)
    check("delta: two keys created", size["keys_created"] == 2)
    check("delta: one value set, one deleted", size["values_set"] == 2 and size["values_deleted"] == 1)
    check("delta survives JSON", json.loads(json.dumps(delta)) == delta)
    replayed, report = apply(base, delta)
    check("replay on the same base gives the application's registry",
          same(replayed, mine) and not report["conflicts"])
    check("replaying twice changes nothing more", same(apply(replayed, delta)[0], mine))

    # A new base: a value the application did not touch changed, a key was
    # added, the value the application changed also changed (a conflict), and
    # the key the application deleted got a new value (a conflict).
    new_base = base.copy()
    new_version = new_base.keys[version.ident]
    new_version.values[fold(unescape("CurrentBuild"))] = "\"CurrentBuild\"=\"26100\""
    new_version.values[fold(unescape("ProductName"))] = "\"ProductName\"=\"Windows 11 Pro\""
    new_base.keys[key_id("Software\\\\Wine\\\\Gone\\\\Deeper")].values[fold(unescape("y"))] = "\"y\"=dword:00000002"
    fresh = Key("Software\\\\Wine\\\\NewInBase", 1800000000)
    fresh.time = "1dc0f0000000000"
    fresh.values[()] = "@=\"new\""
    new_base.keys[fresh.ident] = fresh

    rebased, _delta, report = rebase(base, mine, new_base)
    kinds = sorted(entry["kind"] for entry in report["conflicts"])
    check("rebase: two conflicts, of the right kinds", kinds == ["change-vs-change", "delete-vs-change"])
    check("rebase: the base's untouched change is kept",
          rebased.keys[version.ident].values[fold(unescape("CurrentBuild"))].endswith("\"26100\""))
    check("rebase: the application wins its conflict by default",
          rebased.keys[version.ident].values[fold(unescape("ProductName"))].endswith("\"Changed By App\""))
    check("rebase: the base's new key is there", fresh.ident in rebased.keys)
    check("rebase: the application's key is there", added.ident in rebased.keys)
    check("rebase: the application's deletions hold",
          key_id("Software\\\\Wine\\\\Gone\\\\Deeper") not in rebased.keys
          and fold(unescape("Content Type")) not in rebased.keys[key_id("Software\\\\Classes\\\\.txt")].values)
    check("rebase: the link key survives", rebased.keys[key_id("System\\\\CurrentControlSet")].link)
    check("rebase: the result parses again and is the same",
          same(Registry.parse(rebased.dump()), rebased))

    theirs, _delta, report = rebase(base, mine, new_base, prefer="base")
    check("prefer=base: the base wins the conflict",
          theirs.keys[version.ident].values[fold(unescape("ProductName"))].endswith("\"Windows 11 Pro\"")
          and key_id("Software\\\\Wine\\\\Gone\\\\Deeper") in theirs.keys)

    noise = {(version.ident, fold(unescape("ProductName")))}
    quiet, _delta, report = rebase(base, mine, new_base, noise=noise)
    check("noise: a value Wine itself rewrites is left to the new base",
          quiet.keys[version.ident].values[fold(unescape("ProductName"))].endswith("\"Windows 11 Pro\"")
          and report["noise_skipped"] == 1 and len(report["conflicts"]) == 1)

    # Leaving a group: rebasing back onto the old base undoes nothing of the
    # application's own.
    back, _delta, report = rebase(new_base, rebased, base)
    check("rebase there and back: the application's registry again", same(back, mine))
    print(f"{len(failures)} failure(s)")
    return 1 if failures else 0


def main(argv: list[str]) -> int:
    if len(argv) >= 2 and argv[1] == "--self-test":
        return self_test()
    if len(argv) == 3 and argv[1] == "roundtrip":
        data = pathlib.Path(argv[2]).read_bytes().decode("utf-8", errors="surrogateescape")
        registry = Registry.parse(data)
        print(json.dumps({"identical": registry.dump() == data, **registry.stats()}))
        return 0
    if len(argv) == 3 and argv[1] == "stats":
        print(json.dumps(Registry.load(argv[2]).stats()))
        return 0
    if len(argv) == 5 and argv[1] == "diff":
        delta = diff(Registry.load(argv[2]), Registry.load(argv[3]))
        pathlib.Path(argv[4]).write_text(json.dumps(delta, indent=1))
        print(json.dumps(delta_size(delta)))
        return 0
    noise = None
    if "--noise" in argv:
        at = argv.index("--noise")
        noise = touched(json.loads(pathlib.Path(argv[at + 1]).read_text()))
        argv = argv[:at] + argv[at + 2:]
    if len(argv) == 5 and argv[1] == "apply":
        result, report = apply(Registry.load(argv[2]), json.loads(pathlib.Path(argv[3]).read_text()), noise)
        result.save(argv[4])
        print(json.dumps(report, indent=1))
        return 0
    if len(argv) == 6 and argv[1] == "rebase":
        result, _delta, report = rebase(Registry.load(argv[2]), Registry.load(argv[3]),
                                        Registry.load(argv[4]), noise)
        result.save(argv[5])
        print(json.dumps(report, indent=1))
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
