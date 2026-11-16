#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""What the winsee harnesses share: the X display without a screen, the
screenshot, rectangles drawn over a screenshot so that a person can check
them, the labelled data for the vision test, and the rule by which a point
a vision model proposes is confirmed against what a window exposes.

See docs/reports/exp-winsee-1.md.
"""

import json
import os
import pathlib
import re
import shutil
import subprocess
import time

SCREEN = (1280, 800)

# Roles a person would call a control and could ask for by name.
TARGET_ROLES = {
    "push button": "button",
    "toggle button": "button",
    "check box": "check box",
    "radio button": "radio button",
    "text": "text field",
    "entry": "text field",
    "combo box": "drop-down list",
    "page tab": "tab",
    "list item": "list entry",
    "tree item": "tree entry",
    "menu": "menu",
    "menu item": "menu item",
    "check menu item": "menu item",
    "link": "link",
    "table column header": "column header",
    "table cell": "table cell",
}

# Role groups for comparing what two sources call the same control.
ROLE_GROUPS = {
    "push button": "button",
    "toggle button": "button",
    "check box": "check",
    "radio button": "radio",
    "text": "text",
    "entry": "text",
    "password text": "text",
    "combo box": "combo",
    "page tab": "tab",
    "list item": "item",
    "table cell": "item",
    "tree item": "tree item",
    "menu": "menu",
    "menu item": "menu",
    "check menu item": "menu",
    "radio menu item": "menu",
    "link": "link",
    "table column header": "header",
    "label": "label",
    "static": "label",
    "slider": "slider",
    "spin button": "spin",
}


def norm(name):
    """A name as compared: letter case, mnemonics, a trailing colon or
    ellipsis and repeated spaces do not count."""
    text = (name or "").replace("&", "").replace("…", "...")
    text = re.sub(r"\s+", " ", text).strip().lower()
    return text.rstrip(":. ").strip()


def group(role):
    return ROLE_GROUPS.get(role or "", role or "")


class Display:
    """An X server without a screen, and a window manager if there is one."""

    def __init__(self, logs, number=99):
        self.number = number
        self.name = f":{number}"
        self.logs = pathlib.Path(logs)
        self.logs.mkdir(parents=True, exist_ok=True)
        self.processes = []
        self.window_manager = None

    def up(self):
        for stale in (f"/tmp/.X{self.number}-lock", f"/tmp/.X11-unix/X{self.number}"):
            try:
                os.unlink(stale)
            except OSError:
                pass
        log = open(self.logs / "xvfb.log", "wb")
        self.processes.append(
            subprocess.Popen(
                ["Xvfb", self.name, "-screen", "0", f"{SCREEN[0]}x{SCREEN[1]}x24", "-nolisten", "tcp", "-ac"],
                stdout=log,
                stderr=subprocess.STDOUT,
            )
        )
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline and not os.path.exists(f"/tmp/.X11-unix/X{self.number}"):
            time.sleep(0.2)
        if not os.path.exists(f"/tmp/.X11-unix/X{self.number}"):
            raise RuntimeError("Xvfb did not start")
        for manager in ("openbox", "metacity", "xfwm4", "icewm", "twm"):
            if shutil.which(manager):
                log = open(self.logs / f"{manager}.log", "wb")
                environment = dict(os.environ, DISPLAY=self.name)
                self.processes.append(subprocess.Popen([manager], stdout=log, stderr=subprocess.STDOUT, env=environment))
                self.window_manager = manager
                time.sleep(2)
                break
        if shutil.which("xsetroot"):
            subprocess.run(["xsetroot", "-solid", "#3a4a5a"], env=dict(os.environ, DISPLAY=self.name), check=False)
        return self

    def down(self):
        for process in reversed(self.processes):
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
        self.processes = []

    def shot(self, path):
        """A screenshot of the whole screen. Returns the path, or None."""
        path = pathlib.Path(path)
        path.parent.mkdir(parents=True, exist_ok=True)
        try:
            from PIL import ImageGrab

            image = ImageGrab.grab(xdisplay=self.name)
            image.convert("RGB").save(path)
            return path
        except Exception:  # noqa: BLE001 - the other way is tried
            pass
        if shutil.which("import"):
            done = subprocess.run(
                ["import", "-display", self.name, "-window", "root", str(path)], capture_output=True, timeout=60, check=False
            )
            if done.returncode == 0 and path.exists():
                return path
        return None


COLOURS = {
    "button": (220, 40, 40),
    "check": (220, 120, 0),
    "radio": (220, 120, 0),
    "text": (0, 110, 220),
    "combo": (0, 150, 150),
    "tab": (150, 0, 200),
    "item": (0, 150, 60),
    "tree item": (0, 150, 60),
    "menu": (200, 0, 120),
    "header": (90, 90, 90),
    "label": (120, 120, 120),
}


def annotate(shot, rectangles, out, crop=None):
    """Draw `rectangles` ([x, y, w, h, role, text]) over a screenshot, so
    that a person can see whether they sit on the controls they name."""
    from PIL import Image, ImageDraw

    image = Image.open(shot).convert("RGB")
    draw = ImageDraw.Draw(image)
    for x, y, w, h, role, text in rectangles:
        colour = COLOURS.get(group(role), (255, 0, 255))
        draw.rectangle([x, y, x + w - 1, y + h - 1], outline=colour, width=1)
        if text:
            draw.text((x + 2, max(0, y - 10)), str(text)[:24], fill=colour)
    if crop:
        image = image.crop(crop)
    pathlib.Path(out).parent.mkdir(parents=True, exist_ok=True)
    image.save(out)
    return out


def crop_box(rect, margin=6):
    """The part of the screen a window's rectangle takes, with a margin."""
    x, y, w, h = rect
    left, top = max(0, x - margin), max(0, y - margin)
    right, bottom = min(SCREEN[0], x + w + margin), min(SCREEN[1], y + h + margin)
    return [left, top, right, bottom]


def choose_targets(candidates, box, most=10):
    """The controls of one window the vision test asks for.

    `candidates` are {"name", "role", "rect": [x, y, w, h] on the screen}.
    Kept: a role a person would ask for, a name of 1 to 40 characters with a
    letter or a digit in it, a rectangle of at least 8 by 8 inside `box`,
    and a name that only one kept control of the window has. At most `most`,
    spread over the window from top to bottom."""
    left, top, right, bottom = box
    kept = []
    for candidate in candidates:
        name = (candidate.get("name") or "").strip()
        role = candidate.get("role") or ""
        x, y, w, h = candidate["rect"]
        if role not in TARGET_ROLES or not 1 <= len(name) <= 40 or not re.search(r"[A-Za-z0-9]", name):
            continue
        if w < 8 or h < 8 or x < left or y < top or x + w > right or y + h > bottom:
            continue
        kept.append({"name": name, "role": role, "what": TARGET_ROLES[role], "rect": [x - left, y - top, w, h]})
    counts = {}
    for target in kept:
        counts[norm(target["name"])] = counts.get(norm(target["name"]), 0) + 1
    unique = [target for target in kept if counts[norm(target["name"])] == 1]
    unique.sort(key=lambda target: (target["rect"][1], target["rect"][0]))
    if len(unique) > most:
        step = len(unique) / most
        unique = [unique[int(index * step)] for index in range(most)]
    return unique, len(kept)


def write_capture(directory, label, shot, box, targets, source, labelled_by, window, candidates_seen, extra=None):
    """One labelled window for the vision test: the cropped screenshot and
    what was asked of it."""
    from PIL import Image

    directory = pathlib.Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    image = Image.open(shot).convert("RGB").crop(box)
    image.save(directory / f"{label}.png")
    record = {
        "label": label,
        "image": f"{label}.png",
        "width": image.size[0],
        "height": image.size[1],
        "source": source,
        "labelled_by": labelled_by,
        "window": window,
        "screen_box": box,
        "named_controls_in_window": candidates_seen,
        "targets": targets,
    }
    if extra:
        record.update(extra)
    (directory / f"{label}.json").write_text(json.dumps(record, indent=2))
    # The same crop with the targets drawn, for a person to check.
    annotate(
        directory / f"{label}.png",
        [target["rect"] + [target["role"], target["name"]] for target in targets],
        directory / "checked" / f"{label}.png",
    )
    return record


def confirm_point(requested_name, requested_role, answer):
    """The rule for acting on a point a vision model proposed.

    `answer` is what the tree (or the Win32 tier) says is at that point:
    {"found", "element": {"name", "role", ...}, "topmost": {"same_window"}}.
    Vision proposes; the tree confirms. Only when the control there, read
    from the application, has the name and the kind that was asked for does
    the action go on, and then as an ordinary action on that element, which
    the policy engine classifies by what the tree says. Anything else is
    class 2, which is refused today."""
    if not answer or not answer.get("found"):
        return {"decision": "refuse", "class": 2, "why": "nothing the application exposes is under the point"}
    topmost = answer.get("topmost")
    if topmost is not None and not topmost.get("same_window", True):
        return {"decision": "refuse", "class": 2, "why": "another window covers the point"}
    element = answer.get("element", {})
    if norm(element.get("name")) != norm(requested_name):
        return {
            "decision": "refuse",
            "class": 2,
            "why": "the control under the point is not the one asked for",
            "under_the_point": {"name": element.get("name", ""), "role": element.get("role", "")},
        }
    if requested_role and group(element.get("role")) != group(requested_role):
        return {
            "decision": "refuse",
            "class": 2,
            "why": "the control under the point is of another kind",
            "under_the_point": {"name": element.get("name", ""), "role": element.get("role", "")},
        }
    return {"decision": "confirmed", "element": element.get("id", ""), "why": "the tree names the same control at the point"}
