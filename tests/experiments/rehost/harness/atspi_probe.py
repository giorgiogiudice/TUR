#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""What the accessibility bus (AT-SPI) shows of the applications that are
running: one JSON object on standard output.

Started by harness/run.py inside the test session, after an application has
opened its window. It is a plain AT-SPI client (libatspi through GObject
introspection), as P@x's bridge is one: what it cannot see, P@x cannot see.
"""

import json
import sys
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402

LIMIT = 6000
BUDGET = 25.0
ACTIONABLE = {"push button", "menu item", "link", "entry", "check box", "radio button", "combo box", "page tab", "toggle button", "text", "menu", "list item", "tree item"}


def main():
    started = time.time()
    desktop = Atspi.get_desktop(0)
    applications = []
    for index in range(desktop.get_child_count()):
        try:
            application = desktop.get_child_at_index(index)
            name = application.get_name() or ""
        except Exception as problem:  # GLib.Error from a dying application
            applications.append({"name": "?", "error": str(problem)[:120]})
            continue
        record = {"name": name, "nodes": 0, "named": 0, "actionable": 0, "roles": {}, "sample": [], "windows": [], "truncated": False}
        try:
            record["toolkit"] = f"{application.get_toolkit_name()} {application.get_toolkit_version()}"
        except Exception:
            record["toolkit"] = "?"
        queue = [(application, 0)]
        while queue:
            if record["nodes"] >= LIMIT or time.time() - started > BUDGET:
                record["truncated"] = True
                break
            node, depth = queue.pop(0)
            try:
                role = node.get_role_name()
                label = node.get_name() or ""
                count = node.get_child_count()
            except Exception:
                continue
            record["nodes"] += 1
            record["roles"][role] = record["roles"].get(role, 0) + 1
            if label.strip():
                record["named"] += 1
            if role in ACTIONABLE and label.strip():
                record["actionable"] += 1
                if len(record["sample"]) < 30:
                    record["sample"].append(f"{role}: {label.strip()[:50]}")
            if role in ("frame", "window", "dialog", "alert") and len(record["windows"]) < 8:
                record["windows"].append(f"{role}: {label.strip()[:80]}")
            for child_index in range(min(count, 400)):
                try:
                    child = node.get_child_at_index(child_index)
                except Exception:
                    continue
                if child is not None:
                    queue.append((child, depth + 1))
        record["roles"] = dict(sorted(record["roles"].items(), key=lambda item: -item[1])[:14])
        applications.append(record)
    json.dump({"applications": applications, "seconds": round(time.time() - started, 1)}, sys.stdout)


if __name__ == "__main__":
    main()
