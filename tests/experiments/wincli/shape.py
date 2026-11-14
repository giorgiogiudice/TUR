# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Reduce a command's result to its shape, and compare two shapes.

The judge is real Windows. What is recorded from it is not its text but a
shape: the exit code, the normalised output of commands whose output comes
from our own fixture, the set of field names or table columns, whether each
of a list of patterns matched, and the state of the fixture tree afterwards.
A candidate on Linux is run through the same reduction and the two shapes
are compared field by field.

Comparators (corpus.py attaches one or more to each command):

    text      the normalised output lines, equal
    probes    a list of patterns; the vector of which matched, equal. Every
              probe is written to hold on Windows; one that does not marks
              the check invalid rather than the candidate wrong.
    columns   the header cells of the first table in the output, equal
    keys      the set of "Key : value" names, equal
    json      the output parsed as JSON: its structure, and its values
              unless told otherwise, equal

The exit code is always compared: zero against non-zero, or exactly where
the corpus says the number itself carries meaning. The fixture tree is
always compared.
"""

from __future__ import annotations

import difflib
import json
import re

ANSI = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)")
FIX = re.compile(r"(?i)\bc:\\turfix\b")
RULER = re.compile(r"^\s*[-=]{2,}(\s+[-=]{1,})*\s*$")
KEYLINE = re.compile(r"^\s*(\S.*?)[\s.]*:(?:\s|$)")


def clean(text: str) -> str:
    """Line endings, colour sequences and other noise that carries no meaning."""
    text = ANSI.sub("", text.replace("﻿", ""))
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    return text.replace("\x00", "").replace("\x0c", "")


def norm_text(out: str, spec: dict) -> list[str]:
    flags = set(spec.get("flags", ()))
    lines = []
    for raw in clean(out).split("\n"):
        line = raw.rstrip()
        if any(re.search(rx, line) for rx in spec.get("drop", ())):
            continue
        line = FIX.sub("<FIX>", line)
        for rx, repl in spec.get("mask", ()):
            line = re.sub(rx, repl, line)
        if "ws" in flags:
            line = " ".join(line.split())
        if "ci" in flags:
            line = line.lower()
        if line.strip():
            lines.append(line)
    if "sort" in flags:
        lines.sort()
    return lines


def table_columns(out: str) -> list[str]:
    """Header cells of the first table: the line above the first ruler."""
    lines = [line.rstrip() for line in clean(out).split("\n")]
    for i in range(1, len(lines)):
        if RULER.match(lines[i]) and lines[i - 1].strip():
            header = lines[i - 1]
            spans = [m.span() for m in re.finditer(r"[-=]+", lines[i])]
            # Each cell sits over its ruler (PowerShell rules exactly the
            # label; tasklist rules the column and keeps the label inside).
            cells = [header[start:end].strip() for start, end in spans]
            cells = [c for c in cells if c]
            if "".join(cells).replace(" ", "") != header.replace(" ", ""):
                # The slices lost text: fall back to cells two spaces apart.
                cells = [c for c in re.split(r"\s{2,}", header.strip()) if c]
            return cells
    return []


def kv_keys(out: str) -> list[str]:
    keys = set()
    for line in clean(out).split("\n"):
        m = KEYLINE.match(line)
        if m:
            keys.add(m.group(1).strip())
    return sorted(keys)


def json_shape(value, vals: bool):
    if isinstance(value, dict):
        return {k: json_shape(v, vals) for k, v in sorted(value.items())}
    if isinstance(value, list):
        return [json_shape(v, vals) for v in value]
    if vals:
        return value
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "bool"
    if isinstance(value, (int, float)):
        return "num"
    return "str"


def files_view(files: dict[str, str], mode: str):
    if mode == "skip":
        return None
    if mode == "names":
        return sorted(files)
    return dict(sorted(files.items()))


def compute(entry: dict, result: dict) -> dict:
    """The shape of one result. `result` has exit, stdout, timeout, files."""
    out = result.get("stdout", "")
    shape: dict = {
        "exit": result.get("exit"),
        "timeout": bool(result.get("timeout")),
        "files": files_view(result.get("files", {}), entry.get("files", "hash")),
    }
    for i, spec in enumerate(entry["cmp"]):
        kind = spec["k"]
        key = f"{i}:{kind}"
        if kind == "text":
            shape[key] = norm_text(out, spec)
        elif kind == "probes":
            cleaned = "\n".join(line.rstrip() for line in clean(out).split("\n"))
            shape[key] =[bool(re.search(rx, cleaned, re.M)) for rx in spec["rx"]]
        elif kind == "columns":
            shape[key] = table_columns(out)
        elif kind == "keys":
            shape[key] = kv_keys(out)
        elif kind == "json":
            try:
                shape[key] = json_shape(json.loads(clean(out)), spec.get("vals", True))
            except (ValueError, RecursionError):
                shape[key] = "<not json>"
    return shape


def exit_equal(entry: dict, a, b) -> bool:
    if a is None or b is None:
        return False
    if entry.get("exact"):
        return a == b
    return (a == 0) == (b == 0)


def truth_problem(entry: dict, shape: dict) -> str | None:
    """Why this Windows result cannot serve as ground truth, or None."""
    if shape.get("timeout"):
        return "timed out on Windows"
    for i, spec in enumerate(entry["cmp"]):
        if spec["k"] == "probes" and not all(shape[f"{i}:probes"]):
            bad = [rx for rx, ok in zip(spec["rx"], shape[f"{i}:probes"]) if not ok]
            return "probe does not hold on Windows: " + " ; ".join(bad)
        if spec["k"] == "columns" and not shape[f"{i}:columns"]:
            return "no table found on Windows"
        if spec["k"] == "keys" and not shape[f"{i}:keys"]:
            return "no key/value lines found on Windows"
        if spec["k"] == "json" and shape[f"{i}:json"] == "<not json>":
            return "output is not JSON on Windows"
        if spec["k"] == "text" and not shape[f"{i}:text"] and not spec.get("empty_ok"):
            return "no output on Windows"
    return None


def _ratio(a, b) -> float:
    if a == b:
        return 1.0
    if isinstance(a, list) and isinstance(b, list):
        if a and b and isinstance(a[0], bool):
            n = max(len(a), len(b))
            return sum(1 for x, y in zip(a, b) if x == y) / n if n else 1.0
        return difflib.SequenceMatcher(None, [str(x) for x in a], [str(x) for x in b]).ratio()
    return 0.0


def compare(entry: dict, truth: dict, cand: dict) -> dict:
    """Verdict PASS, PARTIAL or FAIL with what differed."""
    diffs = []
    scores = []
    if cand.get("timeout"):
        return {"verdict": "FAIL", "why": ["timed out"], "fidelity": 0.0}
    exit_ok = exit_equal(entry, truth.get("exit"), cand.get("exit"))
    if not exit_ok:
        diffs.append(f"exit {cand.get('exit')} (Windows {truth.get('exit')})")
    for key in truth:
        if ":" not in key:
            continue
        score = _ratio(truth[key], cand.get(key))
        scores.append(score)
        if score < 1.0:
            diffs.append(key.split(":", 1)[1])
    if truth.get("files") is not None:
        same = truth["files"] == cand.get("files")
        scores.append(1.0 if same else 0.0)
        if not same:
            diffs.append("files")
    fidelity = sum(scores) / len(scores) if scores else 1.0
    if exit_ok and not diffs:
        verdict = "PASS"
    elif exit_ok and fidelity >= 0.5:
        verdict = "PARTIAL"
    else:
        verdict = "FAIL"
    return {"verdict": verdict, "why": diffs, "fidelity": round(fidelity, 3)}
