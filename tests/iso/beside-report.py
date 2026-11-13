#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Judge the "beside Windows on the same disk" runs of the iso workflow
(ADR-0029) from the facts they recorded, and write a summary.

    tests/iso/beside-report.py --job beside --out out/iso-beside
    tests/iso/beside-report.py --job faults --out out/iso-beside-faults

`beside` (one disk laid out as Windows leaves it after "Shrink Volume"):

  refused/          disks on which it is not possible: listed with the reason
                    and code, not offered, unchanged
  shared/           the text installer installs beside Windows; the disk is
                    compared with what it held before after the installation,
                    after TUR's first start and after "Windows" was chosen in
                    TUR's boot menu, which must start the stand-in for Windows
                    Boot Manager from the shared EFI system partition
  live/             the same with the graphical installer, when present

`faults`: the power is cut at each step of an installation beside Windows
(cut-<step>/); after every cut Windows' partitions, its files on the EFI
system partition and its firmware boot entry must be as they were. What the
disk then starts is recorded, not judged.

Writes OUT/summary.md and OUT/beside-report.json; exits 1 if a required check
failed. Run in CI only (ADR-0002).
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys

ESP = "C12A7328-F81F-11D2-BA4B-00A0C93EC93B"
TUR_ESP_PREFIXES = ("EFI/fedora/", "EFI/BOOT/", "EFI/Boot/")


def load(path: pathlib.Path) -> dict | None:
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return None


class Checks:
    def __init__(self):
        self.rows: list[tuple[str, str, bool | None, str]] = []

    def add(self, group: str, what: str, ok: bool | None, detail: str = "") -> None:
        self.rows.append((group, what, ok, detail))

    def failed(self) -> list:
        return [row for row in self.rows if row[2] is False]

    def markdown(self) -> str:
        lines = ["| Run | Check | Result | Detail |", "|---|---|---|---|"]
        for group, what, ok, detail in self.rows:
            mark = "pass" if ok else ("**FAIL**" if ok is False else "recorded")
            lines.append(f"| {group} | {what} | {mark} | {detail.replace('|', '/')[:300]} |")
        return "\n".join(lines)


def esp_partuuid(before: dict | None) -> str | None:
    for part in (before or {}).get("partitions", []):
        if part.get("type") == ESP:
            return part.get("uuid")
    return None


def windows_entry(entries: dict | None) -> dict | None:
    for number, entry in ((entries or {}).get("entries") or {}).items():
        if entry.get("description") == "Windows Boot Manager":
            return dict(entry, number=number)
    return None


def judge_compare(checks: Checks, group: str, compare: dict | None, when: str) -> None:
    if compare is None:
        checks.add(group, f"Windows as it was, {when}", False, "no comparison recorded")
        return
    checks.add(group, f"Windows' partitions and files as they were, {when}", compare.get("windows_intact") is True,
               "; ".join(compare.get("problems", [])) or "every Windows partition byte-identical, every file on "
               "the EFI system partition identical")
    for part in compare.get("windows_partitions", []):
        if "esp_files_before" in part:
            added = part.get("esp_files_added", [])
            foreign = [path for path in added if not path.startswith(TUR_ESP_PREFIXES)]
            checks.add(group, f"files added to the EFI system partition, {when}", not foreign,
                       f"{len(added)} added: {added}; Windows' {part['esp_files_unchanged']} of "
                       f"{part['esp_files_before']} unchanged; fsck.fat -n status {part.get('esp_fsck_status_after')}")
    added = compare.get("added_partitions", [])
    if added:
        checks.add(group, f"new partitions inside the free space, {when}",
                   all(item["inside_free_space_before"] for item in added),
                   "; ".join(f"{item['number']} '{item['name']}' at sector {item['start']}, {item['size']} sectors"
                             for item in added))
    checks.add(group, f"first sector (protective MBR) unchanged, {when}", None, str(compare.get("mbr_unchanged")))


def judge_firmware(checks: Checks, group: str, result: dict | None, require_tur: bool) -> None:
    entries = (result or {}).get("firmware_boot_entries") or {}
    before, after = windows_entry(entries.get("before")), windows_entry(entries.get("after"))
    checks.add(group, "firmware entry \"Windows Boot Manager\" unchanged",
               before is not None and after is not None and before.get("device_path") == after.get("device_path"),
               f"before {before}; after {after}")
    order = (entries.get("after") or {}).get("boot_order_descriptions")
    tur_first = bool(order) and order[0] == "TUR"
    checks.add(group, "firmware boot order after", (tur_first if require_tur else None), str(order))


def beside(out: pathlib.Path, checks: Checks, facts: dict) -> None:
    refused = load(out / "refused" / "result.json")
    if refused is None:
        checks.add("refused", "run", False, "no result")
    else:
        listed = {item.get("name"): item for item in refused.get("facts", {}).get("beside_as_listed", [])}
        codes = sorted(item.get("code") for item in listed.values())
        checks.add("refused", "the run reached its end", refused.get("outcome") == "completed",
                   refused.get("stopped_because", ""))
        checks.add("refused", "both disks listed as Windows, not possible beside, with the reason",
                   len(listed) == 2 and all(item.get("possible") == "no" for item in listed.values())
                   and codes == ["TUR-INSTALL-BESIDE-003", "TUR-INSTALL-BESIDE-004"],
                   "; ".join(f"{name}: {item.get('code')} {item.get('why')}" for name, item in listed.items()))
        shrinks = {item.get("name"): item for item in refused.get("facts", {}).get("shrink_as_listed", [])}
        checks.add("refused", "shrinking Windows not offered either (BitLocker on C:; a full ESP)",
                   bool(shrinks) and all(item.get("possible") == "no" for item in shrinks.values()),
                   "; ".join(f"{name}: {item.get('code')} {item.get('why')}" for name, item in shrinks.items()))
        nexts = refused.get("facts", {}).get("next_screen", {})
        checks.add("refused", "choosing either goes to the erase confirmation, not to beside Windows",
                   bool(nexts) and all(value == "confirm" for value in nexts.values()), str(nexts))
        checks.add("refused", "every disk byte-identical afterwards", refused.get("facts", {}).get("disks_unchanged") is True,
                   "nothing installed" if not refused.get("facts", {}).get("installing_seen") else "an installation started")
        facts["refused"] = {"listed": listed, "next_screen": nexts}

    shared = out / "shared"
    before = load(shared / "before.json")
    facts["esp_partuuid"] = esp_partuuid(before)
    install = load(shared / "install" / "result.json")
    if install is None:
        checks.add("shared", "installation", False, "no result")
        return
    f = install.get("facts", {})
    checks.add("shared", "the installation ran to its end and succeeded",
               install.get("outcome") == "completed" and f.get("result") == "success",
               f"{f.get('result_line', install.get('stopped_because', ''))}; bootc {f.get('install_seconds')} s")
    target = f.get("target_beside") or {}
    checks.add("shared", "the disk was offered for installing beside Windows", target.get("possible") == "yes",
               f"free {target.get('free_size')} bytes at {target.get('free_start')}, after {target.get('after')}, "
               f"before {target.get('before')}; ESP {target.get('esp')} {target.get('esp_free')} of "
               f"{target.get('esp_size')} bytes free, {target.get('esp_needed')} needed")
    confirm = f.get("confirmation") or {}
    checks.add("shared", "the confirmation names the disk (phrase computed by the test)", confirm.get("phrase_matches") is True,
               f"shown {confirm.get('phrase_shown')!r}")
    erase = f.get("erase_phrase_on_beside") or {}
    checks.add("shared", "the disk's erase phrase does not confirm installing beside Windows",
               erase.get("result") == "mismatch", str(erase))
    states = [x.removeprefix("beside ") for x in f.get("beside_states", [])]
    checks.add("shared", "the installer's own check: Windows' files and partition entries unchanged",
               any(state.startswith("windows-files unchanged=yes") for state in states),
               next((s for s in states if s.startswith("windows-files")), "no line"))
    menu = next((s for s in states if s.startswith("boot-menu result=")), "")
    checks.add("shared", "\"Windows\" written into TUR's boot menu, and grub.cfg reads that file",
               "result=yes" in menu and "sourced=yes" in menu, menu)
    checks.add("shared", "the installer's lines", None, " / ".join(states)[:1200])
    checks.add("shared", "TUR chosen to start by default", f.get("default_chosen") == "tur", str(f.get("default_chosen")))
    title = next((s for s in states if s.startswith("step=boot-menu")), "")
    checks.add("shared", "the menu entry is plain \"Windows\" (no BitLocker on this disk)", 'title="Windows"' in title, title)
    runs = f.get("dualboot") or []
    checks.add("shared", "tur-dualboot status / default windows / default tur ran", bool(runs) and all(
        item["status"] == 0 for item in runs), str(runs))
    checks.add("shared", "tur-dualboot's lines", None, " / ".join(f.get("dualboot_lines") or [])[:800])
    restart = f.get("restart_windows") or {}
    checks.add("shared", "Restart into Windows (BootNext): the firmware started Windows Boot Manager's file",
               str(restart.get("standin_file", "")).lower().endswith("\\efi\\microsoft\\boot\\bootmgfw.efi"),
               str(restart))
    checks.add("shared", "... from the shared EFI system partition",
               bool(facts["esp_partuuid"]) and restart.get("standin_partition_uuid") == facts["esp_partuuid"],
               f"{restart.get('standin_partition_uuid')} / ESP {facts['esp_partuuid']}")
    after_vars = (install.get("firmware_boot_entries") or {}).get("after") or {}
    checks.add("shared", "BootNext was used once: gone afterwards, TUR first again",
               after_vars.get("boot_next") is None and (after_vars.get("boot_order_descriptions") or [None])[0] == "TUR",
               f"boot_next {after_vars.get('boot_next')}; order {after_vars.get('boot_order_descriptions')}")
    judge_firmware(checks, "shared", install, require_tur=True)
    judge_compare(checks, "shared", load(shared / "compare-install.json"), "after the installation")

    first = load(shared / "first-start" / "first-start.json")
    checks.add("shared", "TUR starts from the disk (first start, no medium; the start after Restart into Windows)",
               (first or {}).get("outcome") == "rendered",
               f"{(first or {}).get('outcome')}; rendered after {(first or {}).get('rendered_first_seen_seconds')} s")
    judge_compare(checks, "shared", load(shared / "compare-first-start.json"), "after TUR's first start")

    entry = load(shared / "windows-entry" / "first-start.json")
    windows = (entry or {}).get("windows_entry", {})
    checks.add("shared", "\"Windows\" in TUR's boot menu starts EFI/Microsoft/Boot/bootmgfw.efi",
               (entry or {}).get("outcome") == "windows-started"
               and str(windows.get("file", "")).lower().replace("/", "\\").endswith("\\efi\\microsoft\\boot\\bootmgfw.efi"),
               f"{(entry or {}).get('outcome')}; file {windows.get('file')}")
    checks.add("shared", "... from the shared EFI system partition (its partition UUID)",
               bool(facts["esp_partuuid"]) and windows.get("partition_uuid") == facts["esp_partuuid"],
               f"stand-in reports {windows.get('device')}; ESP PARTUUID {facts['esp_partuuid']}")
    judge_compare(checks, "shared", load(shared / "compare-windows-entry.json"), "after Windows was started from TUR's menu")

    shrink = out / "shrink"
    if (shrink / "install" / "result.json").exists():
        result = load(shrink / "install" / "result.json") or {}
        sf = result.get("facts", {})
        checks.add("shrink", "offered for shrinking, within limits", (sf.get("target_shrink") or {}).get("possible") == "yes",
                   str(sf.get("target_shrink")))
        checks.add("shrink", "the erase and beside phrases are refused", all(
            item.get("result") == "mismatch" for item in sf.get("wrong_phrases") or [{}]), str(sf.get("wrong_phrases")))
        checks.add("shrink", "Windows shrunk, then TUR installed beside it",
                   result.get("outcome") == "completed" and sf.get("result") == "success",
                   f"{sf.get('shrink_line')}; {sf.get('result_line', result.get('stopped_because', ''))}")
        judge_compare(checks, "shrink", load(shrink / "compare-install.json"), "after shrinking and installing")
        start = load(shrink / "first-start" / "first-start.json")
        checks.add("shrink", "TUR starts from the disk", (start or {}).get("outcome") == "rendered", str((start or {}).get("outcome")))
    else:
        checks.add("shrink", "the shrink run", False, "no result")

    live = out / "live"
    if (live / "result.json").exists():
        result = load(live / "result.json") or {}
        lf = result.get("facts", {})
        checks.add("live", "the graphical installer installed beside Windows",
                   result.get("outcome") == "completed" and lf.get("result") == "success",
                   f"{lf.get('result_line', result.get('stopped_because', ''))}")
        lv = lf.get("live", {})
        checks.add("live", "the erase phrase leaves the install button unavailable",
                   (lv.get("erase_phrase_on_beside") or {}).get("erase_enabled") == "no", str(lv.get("erase_phrase_on_beside")))
        checks.add("live", "the BESIDE phrase enables it", (lv.get("phrase_typed") or {}).get("erase_enabled") == "yes",
                   str(lv.get("phrase_typed")))
        judge_compare(checks, "live", load(live / "compare-install.json"), "after the installation")
        checks.add("live", "Windows chosen to start by default", lv.get("default_chosen") == "windows",
                   str(lv.get("default_chosen")))
        states = [x.removeprefix("beside ") for x in lf.get("beside_states") or []]
        title = next((s for s in states if s.startswith("step=boot-menu")), "")
        checks.add("live", "the menu entry warns about the BitLocker recovery key (BitLocker's signature on C:)",
                   "BitLocker recovery key" in title, title)
        order = ((result.get("firmware_boot_entries") or {}).get("after") or {}).get("boot_order_descriptions") or []
        checks.add("live", "the firmware's boot order: Windows Boot Manager first, TUR second",
                   order[:2] == ["Windows Boot Manager", "TUR"], str(order))
        start = load(live / "first-start" / "first-start.json") or {}
        entry = start.get("windows_entry") or {}
        checks.add("live", "the first start is Windows (the stand-in, started by the firmware)",
                   start.get("outcome") == "windows-started" and entry.get("partition_uuid") == esp_partuuid(load(live / "before.json")),
                   f"{start.get('outcome')}; {entry.get('file')} {entry.get('partition_uuid')}")
        judge_compare(checks, "live", load(live / "compare-first-start.json"), "after the first start")


def faults(out: pathlib.Path, checks: Checks, facts: dict) -> None:
    before = load(out / "before.json")
    facts["esp_partuuid"] = esp_partuuid(before)
    cuts = sorted(path for path in out.glob("cut-*") if path.is_dir())
    if not cuts:
        checks.add("faults", "cuts", False, "no cut was run")
    facts["cuts"] = {}
    for cut in cuts:
        name = cut.name
        result = load(cut / "result.json") or {}
        info = (result.get("facts") or {}).get("cut") or {}
        landed = (info.get("last_states") or [])[-1:] or ["?"]
        checks.add(name, "the power was cut where intended", bool(info.get("reached_at_seconds")),
                   f"at {info.get('at')} +{info.get('delay_seconds')} s; last state line before the cut: {landed[0]}; "
                   f"{result.get('stopped_because', '')}")
        judge_compare(checks, name, load(cut / "compare.json"), "after the power cut")
        judge_firmware(checks, name, result, require_tur=False)
        start = load(cut / "start" / "first-start.json") or {}
        entry = start.get("windows_entry") or {}
        checks.add(name, "what the disk starts afterwards (key w pressed)", None,
                   f"{start.get('outcome')}; {entry.get('file') or ''} {entry.get('device') or ''}".strip())
        facts["cuts"][name] = {"cut": info, "start": start.get("outcome")}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--job", choices=["beside", "faults"], required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    out = pathlib.Path(args.out)
    checks, facts = Checks(), {}
    (beside if args.job == "beside" else faults)(out, checks, facts)
    failed = checks.failed()
    title = {"beside": "TUR beside Windows on the same disk", "faults": "Power cuts while installing beside Windows"}
    summary = [f"## {title[args.job]} (ADR-0029)", "", checks.markdown(), "",
               f"**{'FAILED: ' + str(len(failed)) + ' check(s)' if failed else 'All required checks passed.'}**", "",
               "The Windows disk is a stand-in made by tests/iso/make-windows-shared-disk.sh: no Microsoft file, "
               "and its bootmgfw.efi is TUR's test program (tests/iso/standin). Real Windows, BitLocker and "
               "Secure Boot were not involved."]
    (out / "summary.md").write_text("\n".join(summary) + "\n")
    (out / "beside-report.json").write_text(json.dumps({"checks": checks.rows, "facts": facts}, indent=2) + "\n")
    print("\n".join(summary))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
