#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Tables from the per-application results of harness/run.py.

    summary.py RESULTS_DIR [RESULTS_DIR...] > results.md

Reads every app-*.json below the given directories. The verdicts in these
tables are the automatic ones; the report's own tables are written after the
screenshots have been looked at.
"""

import glob
import json
import os
import sys


def load(directories):
    records = []
    for directory in directories:
        for path in sorted(glob.glob(os.path.join(directory, "**", "app-*.json"), recursive=True)):
            with open(path, encoding="utf-8") as handle:
                records.append(json.load(handle))
    return records


def cell(value):
    return str(value if value not in (None, "") else "-").replace("|", "/").replace("\n", " ")


def nodes(entry, key="run"):
    pages = ((entry or {}).get(key) or {}).get("pages") or []
    return pages[0].get("nodes") if pages else None


def verdict(entry, key="run", truth=None):
    if entry is None:
        return "no package"
    if key in entry:
        word = entry[key].get("verdict", "?")
        mine, theirs = nodes(entry, key), nodes(truth) if truth else None
        if word == "window" and mine is not None and theirs and mine < 0.5 * theirs:
            return f"window, page incomplete ({mine} of {theirs} elements)"
        if word == "no-window" and mine and mine >= 15:
            return f"no-window (page loaded: {mine} elements)"
        return word
    if key == "pretend":
        return "-"
    if entry.get("error"):
        return "ERROR: " + entry["error"][:60]
    blockers = (entry.get("plan") or {}).get("blockers")
    if blockers:
        return "not re-hosted: " + blockers[0][:70]
    return "NOT RUN"


def natives_text(entry):
    plan = (entry or {}).get("plan") or {}
    counts = {}
    for item in plan.get("natives", []):
        counts[item["status"]] = counts.get(item["status"], 0) + 1
    return ", ".join(f"{count} {status}" for status, count in sorted(counts.items())) or "none"


def main():
    records = load(sys.argv[1:] or ["."])
    out = []
    families = {}
    for record in records:
        families.setdefault(record["family"], []).append(record)

    out.append("## Outcome per application (automatic verdicts)\n")
    out.append("| Family | Application | Official Linux build | Re-hosted from Windows | Re-hosted from macOS | Windows, pretending | macOS, pretending |")
    out.append("|---|---|---|---|---|---|---|")
    for family in sorted(families):
        for record in families[family]:
            variants = record.get("variants", {})
            if record.get("not_run"):
                out.append(f"| {family} | {record['name']} | NOT RUN ({record['not_run']}) | NOT RUN | NOT RUN | - | - |")
                continue
            out.append(
                "| "
                + " | ".join(
                    cell(value)
                    for value in (
                        family,
                        record["name"],
                        verdict(variants.get("linux")),
                        verdict(variants.get("windows"), truth=variants.get("linux")),
                        verdict(variants.get("macos"), truth=variants.get("linux")),
                        verdict(variants.get("windows"), "pretend") if variants.get("windows") else "-",
                        verdict(variants.get("macos"), "pretend") if variants.get("macos") else "-",
                    )
                )
                + " |"
            )

    for family in sorted(families):
        rows = [record for record in families[family] if not record.get("not_run")]
        total = len(rows)
        any_window = sum(1 for record in rows if any(((record["variants"].get(v) or {}).get("run") or {}).get("verdict") in ("window", "works", "ran") for v in ("windows", "macos")))
        windows = sum(1 for record in rows if ((record["variants"].get("windows") or {}).get("run") or {}).get("verdict") in ("window", "works", "ran"))
        macos = sum(1 for record in rows if ((record["variants"].get("macos") or {}).get("run") or {}).get("verdict") in ("window", "works", "ran"))
        truth = sum(1 for record in rows if ((record["variants"].get("linux") or {}).get("run") or {}).get("verdict") in ("window", "works", "ran"))
        out.append(f"\n**{family}**: {total} applications; official Linux build came up: {truth}; re-hosted from Windows: {windows}; from macOS: {macos}; from either: {any_window}.")

    out.append("\n## Detection\n")
    out.append("| Application | Package | Opened as | Detected | Runtime the official Linux build reports |")
    out.append("|---|---|---|---|---|")
    for record in records:
        truth = ((record.get("variants", {}).get("linux") or {}).get("run") or {}).get("electron_running")
        for variant in ("windows", "macos", "linux"):
            entry = record.get("variants", {}).get(variant)
            if not entry:
                continue
            container = entry.get("container") or {}
            problems = "; ".join(container.get("problems") or [])
            out.append(f"| {cell(record['name'])} | {variant}: {cell(entry.get('asset'))} | {cell(' > '.join(container.get('layers') or []))}{' (' + cell(problems[:80]) + ')' if problems else ''} | {cell(entry.get('detected') or entry.get('error'))} | {cell(truth) if variant != 'linux' else '-'} |")

    out.append("\n## Measurements (re-hosted against the official Linux build)\n")
    out.append("| Application | Variant | Verdict | First window (s) | Page ready (s) | PSS (MB) | Window | DOM nodes | AT-SPI nodes / actionable | Typed text arrived | Profile files | Error lines |")
    out.append("|---|---|---|---|---|---|---|---|---|---|---|---|")
    for record in records:
        for variant in ("linux", "windows", "macos"):
            entry = record.get("variants", {}).get(variant) or {}
            for key in ("run", "pretend"):
                data = entry.get(key)
                if not data:
                    continue
                shells = sorted({window["shell"] for window in data.get("windows") or []})
                pages = data.get("pages") or []
                atspi = data.get("atspi") or {}
                out.append(
                    "| "
                    + " | ".join(
                        cell(value)
                        for value in (
                            record["name"],
                            ("official" if variant == "linux" else variant) + (" pretend" if key == "pretend" else ""),
                            data.get("verdict"),
                            data.get("seconds_to_first_window"),
                            data.get("seconds_to_page_ready"),
                            data.get("pss_mb"),
                            ", ".join(shells),
                            pages[0].get("nodes") if pages else ("no DevTools" if data.get("chromium") and not data.get("devtools") else "-"),
                            f"{atspi.get('nodes', '-')} / {atspi.get('actionable', '-')}",
                            data.get("typed_text_arrived"),
                            len(data.get("profile_files") or []),
                            len(data.get("error_lines") or []),
                        )
                    )
                    + " |"
                )

    out.append("\n## Native modules of the Electron payloads\n")
    out.append("| Application | From | Module | Version | In the official Linux build | Outcome | Detail |")
    out.append("|---|---|---|---|---|---|---|")
    totals = {}
    needed = {}
    for record in records:
        linux = (record.get("variants", {}).get("linux") or {}).get("finding") or {}
        linux_modules = {item.get("package") for item in linux.get("natives") or [] if item.get("package")} if linux.get("runtime") == "electron" else None
        for variant in ("windows", "macos"):
            plan = ((record.get("variants", {}).get(variant) or {}).get("plan")) or {}
            for item in plan.get("natives", []):
                totals.setdefault(variant, {}).setdefault(item["status"], 0)
                totals[variant][item["status"]] += 1
                used = "unknown" if linux_modules is None or not item["package"] else ("yes" if item["package"] in linux_modules else "no")
                needed.setdefault(variant, {}).setdefault(used, {}).setdefault(item["status"], 0)
                needed[variant][used][item["status"]] += 1
                out.append(f"| {cell(record['name'])} | {variant} | {cell(item['package'])} | {cell(item['version'])} | {used} | {item['status']} | {cell(item['detail'][:110])} |")
    for variant, groups in needed.items():
        for used, counts in sorted(groups.items()):
            out.append(f"\n**{variant} packages, module in the official Linux build: {used}**: " + ", ".join(f"{count} {status}" for status, count in sorted(counts.items())) + ".")
    for variant, counts in totals.items():
        total = sum(counts.values())
        out.append(f"\n**{variant} packages**: {total} native modules: " + ", ".join(f"{count} {status} ({100 * count // total}%)" for status, count in sorted(counts.items())) + ".")

    out.append("\n## Plans and blockers\n")
    for record in records:
        for variant in ("windows", "macos"):
            entry = record.get("variants", {}).get(variant) or {}
            plan = entry.get("plan")
            if not plan:
                continue
            out.append(f"- **{record['name']}** ({variant}): " + "; ".join(plan.get("steps", []))[:600] + ((" BLOCKERS: " + "; ".join(plan.get("blockers", []))[:400]) if plan.get("blockers") else ""))
            for key in ("run", "pretend"):
                data = entry.get(key) or {}
                if data.get("error_lines"):
                    out.append(f"  - {key} errors: " + " / ".join(data["error_lines"][:3])[:500])
                if key == "pretend" and data.get("shim"):
                    out.append("  - shim: " + "; ".join(line for line in data["shim"] if line.startswith(("stub-called", "load-failed")))[:400])
    print("\n".join(out))


if __name__ == "__main__":
    main()
