#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""What one process exposes on the accessibility bus (AT-SPI), as JSON.

    a11y_dump.py PID

P@x operates applications through the accessibility tree where it can. This
prints how much of an Android application's interface a translation layer
or a container puts there: the number of nodes, their roles, and the names
and texts found.
"""

import collections
import json
import sys

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402


def text_of(node) -> str:
    try:
        iface = node.get_text_iface()
        if iface is None:
            return ""
        n = Atspi.Text.get_character_count(iface)
        return Atspi.Text.get_text(iface, 0, min(n, 200)) or ""
    except Exception:  # noqa: BLE001 - a node without the interface
        return ""


def main() -> int:
    pids = {int(p) for p in sys.argv[1:]}
    Atspi.init()
    desktop = Atspi.get_desktop(0)
    out = {"applications_on_bus": [], "found": False, "nodes": 0, "roles": {}, "names": [], "texts": [], "actions": 0}
    roles = collections.Counter()
    targets = []
    for i in range(desktop.get_child_count()):
        try:
            app = desktop.get_child_at_index(i)
            pid = app.get_process_id()
            out["applications_on_bus"].append({"name": app.get_name(), "pid": pid})
            if pid in pids:
                targets.append(app)
        except Exception as e:  # noqa: BLE001
            out["applications_on_bus"].append({"error": str(e)})
    stack = [(t, 0) for t in targets]
    out["found"] = bool(targets)
    while stack and out["nodes"] < 3000:
        node, depth = stack.pop()
        try:
            out["nodes"] += 1
            roles[node.get_role_name()] += 1
            name = node.get_name() or ""
            desc = node.get_description() or ""
            txt = text_of(node)
            if name and len(out["names"]) < 80:
                out["names"].append(name[:120])
            if desc and len(out["names"]) < 80:
                out["names"].append("desc: " + desc[:120])
            if txt and len(out["texts"]) < 80:
                out["texts"].append(txt[:120])
            try:
                if node.get_action_iface() is not None:
                    out["actions"] += 1
            except Exception:  # noqa: BLE001
                pass
            if depth < 40:
                for j in range(node.get_child_count()):
                    child = node.get_child_at_index(j)
                    if child is not None:
                        stack.append((child, depth + 1))
        except Exception as e:  # noqa: BLE001
            out.setdefault("errors", []).append(str(e)[:120])
    out["roles"] = dict(roles)
    print(json.dumps(out))
    return 0


if __name__ == "__main__":
    sys.exit(main())
