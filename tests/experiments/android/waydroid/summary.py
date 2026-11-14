#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Markdown summary of one Waydroid run (the files run.sh writes).

    summary.py EVIDENCE_DIR TITLE
"""

import pathlib
import sys


def main() -> int:
    ev = pathlib.Path(sys.argv[1])
    title = sys.argv[2] if len(sys.argv) > 2 else "Waydroid"
    out = [f"## {title}\n"]
    for name in ("kernel.txt", "guest-packages.txt", "host-packages.txt"):
        f = ev / name
        if f.exists():
            out += [f"### {name}\n", "```", f.read_text(errors="replace").strip()[:4000], "```\n"]
    facts = ev / "facts.txt"
    if facts.exists():
        out.append("### Facts\n")
        out.append("| Fact | Value |\n|---|---|")
        for line in facts.read_text(errors="replace").splitlines():
            k, _, v = line.partition("=")
            out.append(f"| {k} | {v.replace('|', '/')[:300]} |")
        out.append("")
    else:
        out.append("No facts were written: the script did not reach Waydroid.\n")
    apps = ev / "apps.tsv"
    if apps.exists():
        rows = [line.split("\t") for line in apps.read_text(errors="replace").splitlines() if line.strip()]
        if len(rows) > 1:
            body = rows[1:]
            installed = sum(1 for r in body if r[4] == "Success")
            started = sum(1 for r in body if len(r) > 5 and r[5] == "ok")
            front = sum(1 for r in body if len(r) > 9 and r[0] and r[8].startswith(r[0] + "/") and r[9] == "yes")
            out.append(f"### Applications\n\n{len(body)} applications: {installed} installed, {started} started "
                       f"(`am start -W` status ok), {front} in front and alive at the screenshot. Whether the first "
                       "screen is usable is judged from the screenshots.\n")
            out.append("| " + " | ".join(rows[0]) + " |")
            out.append("|" + "|".join("---" for _ in rows[0]) + "|")
            for r in body:
                out.append("| " + " | ".join(c.replace("|", "/")[:120] for c in r) + " |")
            out.append("")
    for name in ("probe-basic.txt", "probe-bench.txt"):
        f = ev / name
        if f.exists():
            out += [f"### {name}\n", "```", f.read_text(errors="replace").strip()[:5000], "```\n"]
    text = "\n".join(out)
    (ev / "results.md").write_text(text)
    print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
