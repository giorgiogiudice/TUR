#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Tables from the layers experiment's JSON, as Markdown on standard output.

    report.py EVIDENCE_DIR

Reads results.json and fsmatrix*.json there (and under vm/ when that is the
evidence of the virtual machine). Prints what is there; a missing part is
"NOT RUN", never a guess.
"""

from __future__ import annotations

import json
import pathlib
import statistics
import sys

STAGES = ("install", "launch", "check", "uninstall")


def load(path: pathlib.Path):
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError):
        return None


def mb(value) -> str:
    return "n/a" if value is None else f"{value / 1e6:.1f} MB"


def seconds(value) -> str:
    return "n/a" if value is None else f"{value:.2f} s"


def table(header: list[str], rows: list[list]) -> list[str]:
    lines = ["| " + " | ".join(header) + " |", "|" + "---|" * len(header)]
    for row in rows:
        lines.append("| " + " | ".join(str(cell).replace("|", "\\|").replace("\n", " ") for cell in row) + " |")
    return lines + [""]


def cell(app: dict, name: str) -> str:
    entry = app.get(name, {})
    status = entry.get("status", "not-run")
    if status == "not-run":
        return "not run"
    if name == "install" and "seconds" in entry:
        return f"{status} ({entry['seconds']:.1f} s)"
    if name == "launch" and "seconds_to_window" in entry:
        return f"{status} (window {entry['seconds_to_window']:.1f} s)"
    if name == "check" and "seconds" in entry:
        return f"{status} ({entry['seconds']:.1f} s)"
    return status


def passes(results: dict) -> list[str]:
    lines: list[str] = []
    for key in sorted(results):
        if not key.startswith("pass:"):
            continue
        data = results[key]
        lines += [f"#### Pass `{key[5:]}`: {data.get('kind')} prefixes, {data.get('wine')}"
                  + (f", base `{data['base']}`" if data.get("base") else ""), ""]
        rows = []
        for app in data.get("apps", []):
            notes = "; ".join(f"{name}: {app[name].get('detail')}" for name in STAGES
                              if app.get(name, {}).get("status") in ("fail", "partial") and app[name].get("detail"))
            if app.get("harness_error"):
                notes += f" harness error: {app['harness_error']}"
            rows.append([app.get("prefix"), *(cell(app, name) for name in STAGES),
                         mb(app.get("disk", {}).get("apparent_bytes")), notes[:500]])
        lines += table(["Prefix", "Install", "Launch", "Check", "Uninstall", "Own bytes", "Notes"], rows)
    return lines


def matrix(report: dict | None, title: str) -> list[str]:
    if not report:
        return [f"### {title}", "", "NOT RUN", ""]
    lines = [f"### {title}", "", f"Kernel `{report.get('kernel')}`, {report.get('bubblewrap')}, "
             f"{report.get('mount_version')}. overlay module defaults: "
             + ", ".join(f"{key.rsplit('/', 1)[1]}={value}" for key, value in report.items()
                         if key.startswith("/sys/module/overlay")), ""]
    rows = []
    for case in report.get("cases", []):
        if "status" in case:
            rows.append([case["case"], case["status"], "", "", "", ""])
            continue
        mount = case.get("mount", {})
        ops = case.get("ops", {})
        failed = [f"{name} ({entry.get('error')})" for name, entry in ops.items() if not entry.get("ok")]
        case_ops = {name: entry.get("value") for name, entry in ops.items() if name.startswith("case:")}
        rows.append([case["case"],
                     "mounted" if mount.get("ok") else f"refused: {mount.get('stderr', '')[:120]}",
                     "ok: " + case["bubblewrap"].get("stdout", "").replace("\n", " ")[-40:]
                     if case.get("bubblewrap", {}).get("ok")
                     else "failed: " + case.get("bubblewrap", {}).get("stderr", "")[:120],
                     "; ".join(failed) or ("none" if mount.get("ok") else ""),
                     "; ".join(f"{name[6:]}: {value}" for name, value in case_ops.items())[:400],
                     " / ".join(case.get("kernel_messages", [])[-2:])[:300]])
    lines += table(["Arrangement", "Mount in a user namespace", "bubblewrap --overlay", "Operations that failed",
                    "Case", "Kernel said"], rows)
    timing = []
    for case in report.get("cases", []):
        ops = case.get("ops", {})
        if not ops:
            continue

        def value(name: str):
            entry = ops.get(name, {})
            return entry.get("value", entry.get("error", "n/a")) if entry else "n/a"

        def took(name: str):
            entry = ops.get(name, {})
            return f"{entry['seconds'] * 1000:.0f} ms" if entry.get("ok") else entry.get("error", "n/a")
        timing.append([case["case"], took("chmod of a 64 MiB lower file (copy up)"),
                       took("opening a 64 MiB lower file for writing, writing nothing (copy up)"),
                       value("stat of 2000 lower files through the overlay, first pass (microseconds each)"),
                       value("the same, second pass"), value("the same files on the lower layer itself"),
                       value("2000 names that are not there, through the overlay (microseconds each)"),
                       "allowed" if case.get("second_mount_of_same_upper", {}).get("ok") else "refused"])
    lines += table(["Arrangement", "Copy up 64 MiB (chmod)", "Copy up 64 MiB (open for writing)",
                    "stat via overlay, first (µs)", "second (µs)", "on the lower layer (µs)", "missing name (µs)",
                    "Second mount of the same upper"], timing)
    return lines


def h1(results: dict) -> list[str]:
    data = results.get("h1")
    if not data:
        return ["### H1: prefix against layer", "", "NOT RUN", ""]
    lines = ["### H1: prefix against layer", "", f"{data.get('wine')}.", ""]
    rows, startup = [], []
    for name, entry in data.get("variants", {}).items():
        if "status" in entry:
            rows.append([name, entry["status"]] + [""] * 8)
            continue
        first, second, base = entry.get("plain_first", {}), entry.get("plain_second", {}), entry.get("base", {})
        layers = entry.get("layers", [])
        run = entry.get("layer_first_run", {})
        rows.append([
            name + (" (drive C: case-folded)" if entry.get("casefold") else ""),
            f"{seconds(first.get('seconds'))}, {seconds(second.get('seconds'))}",
            mb(second.get("disk", {}).get("apparent_bytes")),
            mb(base.get("disk", {}).get("apparent_bytes")),
            (f"{min(layer['seconds'] for layer in layers) * 1000:.1f} to "
             f"{max(layer['seconds'] for layer in layers) * 1000:.1f} ms") if layers else "n/a",
            f"{layers[0]['bytes']} B ({layers[0]['allocated']} allocated)" if layers else "n/a",
            f"exit {run.get('exit')}, {seconds(run.get('seconds'))}" if run else "n/a",
            mb(run.get("disk", {}).get("apparent_bytes")) if run else "n/a",
            f"{seconds(entry.get('copy_plain', {}).get('seconds'))}, "
            f"{mb(entry.get('copy_plain', {}).get('disk', {}).get('allocated_bytes'))} allocated",
            entry.get("verify_base", {}).get("unchanged", "n/a"),
            entry.get("harness_error", "")])
        samples = entry.get("startup", {})
        line = [name]
        for kind in ("plain", "layer"):
            for field in ("program_seconds", "seconds"):
                values = [sample[field] for sample in samples.get(kind, [])
                          if sample.get("exit") == 0 and sample.get(field) is not None]
                line.append(f"{statistics.median(values):.2f} ({min(values):.2f} to {max(values):.2f}), n={len(values)}"
                            if values else "n/a")
        startup.append(line)
    lines += table(["File system", "Plain prefix: create (two)", "Plain prefix bytes", "Base bytes",
                    "Layer: create (five)", "Layer bytes", "Wine's first start in a layer",
                    "Layer bytes after it", "Copy of a plain prefix", "Base unchanged", "Error"], rows)
    lines += ["`wine cmd /c exit`, alternating plain and layered, seconds, median (range). \"Program\" is until "
              "the program returned; \"all\" includes the Wine server's exit.", ""]
    lines += table(["File system", "Plain: program", "Plain: all", "Layer: program", "Layer: all"], startup)
    return lines


def rebase(results: dict) -> list[str]:
    data = results.get("rebase")
    update = results.get("update", {})
    lines = ["### H3: registry rebase across a Wine version change", ""]
    if not data:
        return lines + ["NOT RUN", ""]
    lines += [f"Layers made under `{data.get('old_wine')}`, rebased and run under `{data.get('wine')}`.", ""]
    if update:
        rows = []
        for label, key in (("old to updated", "registry_old_to_updated"), ("old to fresh", "registry_old_to_fresh"),
                           ("updated to fresh", "registry_updated_to_fresh")):
            for name, size in update.get(key, {}).items():
                rows.append([label, name, size["keys_created"], size["keys_deleted"], size["keys_changed"],
                             size["values_set"], size["values_deleted"]])
        lines += table(["Bases", "File", "Keys created", "Keys deleted", "Keys changed", "Values set",
                        "Values deleted"], rows)
        rows = [[label, entry.get("exit"), seconds(entry.get("seconds")), extra]
                for label, entry, extra in (
                    ("the base (once per machine)", update.get("update_base", {}), ""),
                    ("a plain prefix with 7-Zip (today, per prefix)", update.get("update_plain", {}),
                     f"{mb(update.get('update_plain', {}).get('bytes_before'))} to "
                     f"{mb(update.get('update_plain', {}).get('bytes_after'))}"),
                    ("a layer over the old base (naive)", update.get("update_layer_naively", {}),
                     f"layer {mb(update.get('update_layer_naively', {}).get('bytes_before'))} to "
                     f"{mb(update.get('update_layer_naively', {}).get('bytes_after'))}")) if entry]
        lines += table(["`wineboot --update` of", "Exit", "Time", "Bytes"], rows)
        for label, key in (("old to updated", "files_old_to_updated"), ("old to fresh", "files_old_to_fresh")):
            if key in update:
                lines += [f"Files, {label}: {update[key]}", ""]
    for target, entry in data.get("targets", {}).items():
        lines += [f"#### Onto the `{target}` base", ""]
        if "apps" not in entry:
            lines += [str(entry.get("status")), ""]
            continue
        lines += [f"Noise pairs left to the base: {entry.get('noise_pairs')}", ""]
        rows = []
        for app, found in entry["apps"].items():
            if "delta" not in found:
                rows.append([app, found.get("status")] + [""] * 7)
                continue
            system = found["delta"].get("system.reg", {})
            user = found["delta"].get("user.reg", {})
            status = next((item for item in entry.get("pass", []) if item["id"] == app), {})
            rows.append([app,
                         f"{system.get('keys_created', 0)}+{user.get('keys_created', 0)} keys, "
                         f"{system.get('values_set', 0)}+{user.get('values_set', 0)} values",
                         "; ".join(f"{name}: {info['applied']} applied, {info['noise_skipped']} noise"
                                   for name, info in found.get("registry_files", {}).items()),
                         found.get("conflicts"),
                         f"{found.get('stale_files_dropped')} ({mb(found.get('stale_bytes'))})",
                         f"{len(found.get('kept_modified', [])) + len(found.get('both_changed', []))}"
                         f" (+{len(found.get('hides_new_base_file', []))} hiding a new base file)",
                         seconds(found.get("rebase_seconds")),
                         f"{status.get('launch', 'n/a')} / {status.get('check', 'n/a')}",
                         "; ".join(found.get("registry_complaints", []))[:200] or "none"])
        lines += table(["Application", "Its changes (system+user)", "Replayed", "Conflicts", "Stale files dropped",
                        "Base files it changed", "Rebase time", "Launch / check after", "Wine's complaints"], rows)
        for app, found in entry["apps"].items():
            for conflict in found.get("conflict_list", [])[:12]:
                lines.append(f"- `{app}` {conflict['file']} {conflict['kind']}: `{conflict['key']}` "
                             f"`{conflict.get('value')}`: mine `{conflict.get('mine')}`, base `{conflict.get('base')}`")
        lines += ["", f"Base after: {entry.get('verify_base')}", ""]
    return lines


def rest(results: dict) -> list[str]:
    lines: list[str] = []
    for name, title in (("preflight", "Preflight"), ("disk", "Ten applications: disk"),
                        ("groups", "H4: groups"), ("safety", "H5: safety"), ("extras", "Extras"),
                        ("remove-layered", "Removal"), ("mini", "Applications on layers, per file system")):
        data = results.get(name)
        lines += [f"### {title}", ""]
        if data is None:
            lines += ["NOT RUN", ""]
            continue
        text = json.dumps(data, indent=1, default=str)
        if len(text) > 12000:
            text = text[:12000] + "\n... (cut; the whole is in results.json)"
        lines += ["```json", text, "```", ""]
    old = results.get("old-base")
    if old:
        lines += ["### The old base", "", "```json",
                  json.dumps({key: old.get(key) for key in ("wine", "roundtrip", "verify_base", "phase_error")},
                             indent=1, default=str),
                  json.dumps({"base": {key: old.get("base", {}).get(key) for key in
                                       ("create_seconds", "wineboot_seconds", "disk", "registry_bytes",
                                        "profile_links", "casefolded")},
                              "noise": {key: old.get("noise", {}).get(key) for key in ("captured", "disk", "runs")}},
                             indent=1, default=str)[:6000], "```", ""]
    errors = {name: data.get("phase_error") for name, data in results.items()
              if isinstance(data, dict) and data.get("phase_error")}
    lines += ["### Phases that ended with a harness error", "", str(errors or "none"), ""]
    return lines


def main() -> int:
    evidence = pathlib.Path(sys.argv[1])
    out: list[str] = []
    for label, directory in (("Fedora containers on the runner's kernel", evidence),
                             ("Virtual machine on Fedora 45's kernel", evidence / "vm")):
        results = load(directory / "results.json")
        runner_matrix = load(directory / "fsmatrix-runner-kernel.json") or load(directory / "fsmatrix.json")
        if results is not None and set(results) <= {"download"}:
            results = None               # only the installers were fetched here
        if results is None and runner_matrix is None:
            continue
        out += [f"## {label}", ""]
        not_run = directory / "not-run.txt"
        if not_run.is_file():
            out += [not_run.read_text().strip(), ""]
        out += matrix(runner_matrix, "overlayfs without privilege, by file system and case folding")
        if results:
            out += h1(results) + passes(results) + rebase(results) + rest(results)
    print("\n".join(out) if out else "No results were written.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
