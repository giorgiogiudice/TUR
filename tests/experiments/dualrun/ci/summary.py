#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Turn the experiment's results.json into results.md (tables first, then each scenario's record).

    summary.py OUT_DIR
"""

from __future__ import annotations

import json
import pathlib
import sys


def cell(value) -> str:
    if value is None:
        return "–"
    if isinstance(value, bool):
        return "yes" if value else "no"
    text = json.dumps(value) if isinstance(value, (dict, list)) else str(value)
    return text.replace("|", "\\|").replace("\n", " ")[:260]


def table(header: list[str], rows: list[list]) -> list[str]:
    lines = ["| " + " | ".join(header) + " |", "|" + "---|" * len(header)]
    lines += ["| " + " | ".join(cell(v) for v in row) + " |" for row in rows]
    return lines + [""]


def write(out: pathlib.Path) -> None:
    results = json.loads((out / "results.json").read_text())
    meta, scenarios, sessions = results["meta"], results["scenarios"], results["sessions"]
    lines = ["# dualrun: results of this run", "",
             "Both configurations are QEMU/KVM virtual machines on a CI runner (nested virtualisation). "
             "\"Native\" means the stand-in PC configuration, not a real computer.", ""]
    media = meta.get("media", {})
    lines += table(["Item", "Value"], [
        ["Run", f"{meta.get('run')} at commit {str(meta.get('commit'))[:10]}, dispatched by {meta.get('dispatched_by')}"],
        ["Started, finished (UTC)", f"{meta.get('started_utc')}, {meta.get('finished_utc')}"],
        ["Medium", f"{media.get('name')} ({media.get('bytes')} bytes, SHA-256 {media.get('sha256')})"],
        ["Host", f"{meta.get('cpu')}, {meta.get('host_cpus')} processors, {meta.get('host_memory_mb')} MiB, kernel {meta.get('kernel')}"],
        ["QEMU", f"{meta.get('qemu')}; devices: {meta.get('qemu_has')}"],
        ["Device-mapper targets", ", ".join(meta.get("dm_targets", []))],
        ["Firmware", meta.get("firmware")],
    ])

    install = scenarios.get("install", {})
    lines += ["## Installation", ""]
    lines += table(["Item", "Value"], [[k, v] for k, v in install.items() if k not in ("images_on_medium",)])

    lines += ["## Every start", ""]
    rows = []
    for s in sessions:
        facts = s.get("facts") or {}
        product = facts.get("product") or {}
        rows.append([
            s.get("tag"), s.get("kind"), s.get("mode", ""), s.get("identity", ""), s.get("tpm", ""),
            s.get("start_to_agent_s"), s.get("end"), s.get("end_seconds"),
            "forced" if s.get("forced_off") else "",
            (s.get("attach") or {}).get("refusals") or "",
            product.get("name"), product.get("uuid"), facts.get("licence_status"), facts.get("grace_minutes"),
            f"{facts.get('devices_present')}/{facts.get('devices_known')}" if facts else None,
            len(facts.get("devices_with_problems") or []) if facts else None,
            sum((facts.get("pnp_events") or {}).values()) if facts else None,
            s.get("tur_partition_unchanged"),
            (s.get("detach") or {}).get("partition_table", {}) and (s["detach"]["partition_table"] or {}).get("guest_changed_its_copy"),
        ])
    lines += table(["Start", "Kind", "Mode", "Identity", "TPM", "Start to agent (s)", "Ended by", "End (s)", "Forced",
                    "Refusals", "Model reported", "UUID reported", "Licence status", "Grace (min)",
                    "Devices present/known", "Devices with problems", "Device-install events",
                    "TUR's partition unchanged", "Partition table written by guest"], rows)

    lines += ["## Volumes as Windows lists them", ""]
    rows = []
    for s in sessions:
        facts = s.get("facts") or {}
        if not facts.get("partitions"):
            continue
        letters = ", ".join(f"{p.get('number')}:{p.get('letter') or '-'}({p.get('type')})"
                            for p in sorted(facts["partitions"], key=lambda p: (p.get("disk", 0), p.get("number", 0))))
        labels = ", ".join(f"{v.get('letter') or '-'}:'{v.get('label')}'/{v.get('fs')}" for v in facts.get("volumes", [])
                           if v.get("type") == "Fixed")
        disks = ", ".join(f"{d.get('name')} [{d.get('bus')}] serial {d.get('serial')}" for d in facts.get("disks", []))
        rows.append([s.get("tag"), letters, labels, disks])
    lines += table(["Start", "Partitions: number:letter(type)", "Fixed volumes: letter:'label'/filesystem", "Disks"], rows)

    for name, record in scenarios.items():
        if name == "install":
            continue
        lines += [f"## Scenario `{name}`", ""]
        if "not_run" in record:
            lines += [f"**NOT RUN:** {record['not_run']}", ""]
            continue
        lines += ["```json", json.dumps(record, indent=1, default=str)[:60000], "```", ""]
    (out / "results.md").write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    write(pathlib.Path(sys.argv[1]))
