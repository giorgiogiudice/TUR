#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Turn the evidence of an installer-medium test into iso-report.json and a
summary.

    tests/iso/report.py --job product|proof --out OUT_DIR --expected-build-id ID
                        [--timing NAME=SECONDS ...]

Evidence, all optional (a missing piece fails the checks that need it):

    OUT_DIR/medium/*.manifest.json     installer/iso/build-medium.sh
    OUT_DIR/<scenario>/result.json     tests/iso/install.py, one directory per run
    OUT_DIR/<scenario>/host/           tests/boot/inspect-disk.py installed
    OUT_DIR/<scenario>/first-start/    tests/iso/boot-installed.py and
                                       tests/boot/inspect-disk.py postmortem
    OUT_DIR/layout-comparison.json     tests/iso/compare-layout.py
    OUT_DIR/boot/boot-report.json      tests/boot/report.py
    OUT_DIR/rollback/rollback-report.json   tests/boot/rollback.py

The two jobs of the `iso` workflow:

  product   the medium that carries the product image: what a person would
            write to a USB stick. Refusal tests, an installation onto a blank
            disk, one onto a disk that holds data, the first start of an
            installed disk, an installation onto a second disk beside a
            Windows-like disk, and (informational) the same under Secure Boot.
  proof     a medium built the same way that carries the boot-test image, so
            that the boot test's own checks and a boot-health scenario can run
            on a disk installed from a medium.

Writes OUT_DIR/iso-report.json and OUT_DIR/summary.md, prints the summary and
exits 0 only if every required check passed. A check that could not be
evaluated is "not-run" and counts as a failure when required.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

EXPECTED_ROLES = ["bios-boot", "esp", "root"]


def load(path: pathlib.Path):
    try:
        return json.loads(path.read_text(errors="replace"))
    except (OSError, ValueError):
        return None


class Checks:
    def __init__(self):
        self.items: list[dict] = []

    def add(self, ident: str, description: str, passed, detail: str, required: bool = True) -> None:
        result = "not-run" if passed is None else "pass" if passed else "fail"
        self.items.append(
            {"id": ident, "description": description, "required": required, "result": result, "detail": str(detail)}
        )

    def passed(self) -> bool:
        return all(item["result"] == "pass" for item in self.items if item["required"])


def no_network(command: list[str] | None) -> bool | None:
    if not command:
        return None
    joined = " ".join(command)
    return "-nic none" in joined and "-netdev" not in command and "virtio-net" not in joined


def check_medium(out: pathlib.Path, checks: Checks, args) -> dict:
    manifests = sorted((out / "medium").glob("*.manifest.json"))
    manifest = load(manifests[0]) if manifests else None
    payload = (manifest or {}).get("payload") or {}
    env = (manifest or {}).get("installer_environment") or {}
    medium = (manifest or {}).get("medium") or {}
    source = (manifest or {}).get("source") or {}
    oci = payload.get("oci_layout") or {}
    checks.add(
        "medium.built",
        "The medium was built and described: file, size, SHA-256",
        bool(medium.get("sha256") and medium.get("bytes")) if manifest else None,
        f"{medium.get('file')}: {medium.get('bytes')} bytes, sha256 {medium.get('sha256')}" if manifest else "no manifest",
    )
    checks.add(
        "medium.payload",
        "The image on the medium is the image built from this commit by scripts/build-image.sh",
        (
            bool(payload.get("image_id"))
            and oci.get("config_digest") == f"sha256:{payload.get('image_id')}"
            and source.get("build_id") == args.expected_build_id
        )
        if manifest
        else None,
        f"image ID {payload.get('image_id')}; config on the medium {oci.get('config_digest')}; "
        f"build ID {source.get('build_id')}, expected {args.expected_build_id}; base {payload.get('base_image')}@{payload.get('base_digest')}",
    )
    changed = env.get("packages_changed") or []
    added = [item.get("name") for item in env.get("packages_added") or []]
    checks.add(
        "medium.environment",
        "The installer environment is the payload plus dracut-live: nothing `bootc install` reads differs",
        (
            env.get("files_bootc_install_reads_identical_to_payload") is True
            and "dracut-live" in added
            and all(re.fullmatch(r"dracut(-.*)?", item.get("name", "")) for item in changed)
            and not env.get("packages_removed")
        )
        if manifest
        else None,
        f"added: {added}; changed: {[item.get('name') for item in changed]}; "
        f"install-relevant files identical: {env.get('files_bootc_install_reads_identical_to_payload')}",
    )
    chain = ((manifest or {}).get("boot_chain") or {}).get("files") or []
    names = {item.get("file") for item in chain if item.get("sha256")}
    checks.add(
        "medium.boot_chain",
        "The boot chain on the medium is recorded by hash and package: shim, MOK manager, GRUB, kernel",
        {"shimx64.efi", "mmx64.efi", "grubx64.efi", "vmlinuz"} <= names if manifest else None,
        "; ".join(f"{item.get('file')}: {item.get('package')} sha256 {str(item.get('sha256'))[:16]}..." for item in chain) or "no data",
    )
    return manifest or {}


def check_layout(
    prefix: str, label: str, directory: pathlib.Path, checks: Checks, args, target: str | None, required: bool = True
) -> dict:
    layout = load(directory / "host" / "disk-layout.json")
    parts = (layout or {}).get("partitions") or []
    roles = [part.get("role") for part in parts]
    by_role = {part.get("role"): part for part in parts}
    checks.add(
        f"{prefix}.layout",
        f"{label}: the disk has the layout the boot test installs: GPT, BIOS boot, EFI system partition (FAT), "
        "one ext4 root, no separate /boot",
        (
            layout.get("label") == "gpt"
            and roles == EXPECTED_ROLES
            and (by_role.get("root") or {}).get("filesystem") == "ext4"
            and (by_role.get("esp") or {}).get("filesystem") == "vfat"
            and layout.get("separate_boot_partition") is False
            and not layout.get("inspection_errors")
        )
        if layout
        else None,
        "; ".join(
            f"{p.get('number')}: {p.get('name')} {p.get('role')} {p.get('filesystem')} {round((p.get('size_bytes') or 0) / 2**20)} MiB"
            for p in parts
        )
        or "no data",
        required,
    )
    options = (layout or {}).get("boot_entry_options") or []
    checks.add(
        f"{prefix}.boot_counting",
        f"{label}: what boot counting needs is on the disk (ADR-0014): the counting fragment before the menu in "
        "grub.cfg, the GRUB variable in the loader entry, a 1024-byte environment block on the ext4 root",
        (
            "08_tur_boot_count.cfg" in (layout.get("grub_cfg_fragments") or [])
            and layout.get("grub_cfg_counting_fragment_before_menu") is True
            and any("$tur_boot_args" in option for option in options)
            and layout.get("grubenv_bytes") == 1024
        )
        if layout
        else None,
        f"fragments {(layout or {}).get('grub_cfg_fragments')}; counting before menu: "
        f"{(layout or {}).get('grub_cfg_counting_fragment_before_menu')}; options {options}; "
        f"grubenv {(layout or {}).get('grubenv_bytes')} bytes",
        required,
    )
    aleph = load(directory / "host" / "bootc-aleph.json")
    labels = (aleph or {}).get("labels") or {}
    checks.add(
        f"{prefix}.image",
        f"{label}: bootc's install record names the image built from this commit",
        (
            labels.get("org.tur.build-id") == args.expected_build_id
            and (target is None or (aleph.get("target-image") or aleph.get("image") or "").startswith(target))
        )
        if aleph
        else None,
        f"build ID {labels.get('org.tur.build-id')}, expected {args.expected_build_id}; image {(aleph or {}).get('image')}; "
        f"recorded origin {(aleph or {}).get('target-image')}, expected {target}; SELinux at install: {(aleph or {}).get('selinux')}",
        required,
    )
    return layout or {}


def check_install(name: str, label: str, out: pathlib.Path, checks: Checks, args, manifest: dict, required: bool = True) -> dict:
    directory = out / name
    result = load(directory / "result.json")
    facts = (result or {}).get("facts") or {}
    checks_seen = facts.get("checks") or {}

    def add(ident: str, description: str, passed, detail: str, need: bool = required) -> None:
        checks.add(f"{name}.{ident}", f"{label}: {description}", passed, detail, need)

    add(
        "started_offline",
        "the medium started under UEFI with KVM and no network device, and the installer came up",
        (
            result.get("outcome") in ("completed", "stopped")
            and bool((result.get("kvm") or {}).get("enabled"))
            and no_network(result.get("command")) is True
            and checks_seen.get("boot_mode") == "uefi"
            and "start" in facts
        )
        if result
        else None,
        f"outcome {(result or {}).get('outcome')}; medium as {(result or {}).get('medium_attached_as')}; "
        f"KVM {(result or {}).get('kvm')}; no network device: {no_network((result or {}).get('command'))}; "
        f"boot mode {checks_seen.get('boot_mode')}; installer up after {facts.get('installer_started_seconds')} s; "
        f"kernel command line: {facts.get('kernel_command_line')}",
    )
    add(
        "payload_verified",
        "the installer checked every file of the image on the medium against its SHA-256",
        (checks_seen.get("payload") == "verified" and checks_seen.get("image_id") == (manifest.get("payload") or {}).get("image_id"))
        if result
        else None,
        f"payload: {checks_seen.get('payload')}; image ID on the medium {checks_seen.get('image_id')}; "
        f"in the manifest {(manifest.get('payload') or {}).get('image_id')}",
    )
    confirmation = facts.get("confirmation") or {}
    target = facts.get("target_as_listed") or {}
    add(
        "confirmation",
        "the disk was offered by its stable path and the confirmation phrase names it",
        (
            target.get("status") == "ok"
            and (target.get("by_id") or "").startswith("/dev/disk/by-id/")
            and confirmation.get("phrase_matches") is True
            and confirmation.get("by_id") == target.get("by_id")
        )
        if result
        else None,
        f"listed as {target}; phrase shown {confirmation.get('phrase_shown')!r}, computed by the test "
        f"{confirmation.get('phrase_computed_by_the_test')!r}",
    )
    add(
        "installed",
        "the installation finished without error and the installer said so",
        (facts.get("install_status") == 0 and facts.get("result") == "success" and result.get("outcome") == "completed")
        if result
        else None,
        f"bootc exit status {facts.get('install_status')} after {facts.get('install_seconds')} s; installer result "
        f"{facts.get('result')}; command: {facts.get('install_command')}",
    )
    add(
        "powered_off",
        "the machine powered off from the last screen",
        facts.get("powered_off_by_installer") if result and "powered_off_by_installer" in facts else None,
        f"powered off by the installer: {facts.get('powered_off_by_installer')}",
    )
    shots = (result or {}).get("screenshots") or []
    add(
        "screenshots",
        "a screenshot was taken of every installer screen",
        ({shot.get("screen") for shot in shots} >= {"welcome", "disks", "confirm", "installing", "done"}) if result else None,
        ", ".join(str(shot.get("file")) for shot in shots) or "none",
    )
    layout = check_layout(
        name, label, directory, checks, args, (manifest.get("payload") or {}).get("recorded_origin"), required
    )
    return {
        "name": name,
        "outcome": (result or {}).get("outcome"),
        "medium_attached_as": (result or {}).get("medium_attached_as"),
        "secure_boot_firmware": (result or {}).get("secure_boot_firmware"),
        "secure_boot_seen_by_installer": checks_seen.get("secure_boot"),
        "installer_started_seconds": facts.get("installer_started_seconds"),
        "welcome_seconds": facts.get("welcome_seconds"),
        "install_seconds": facts.get("install_seconds"),
        "elapsed_seconds": (result or {}).get("elapsed_seconds"),
        "root_used_bytes": layout.get("root_used_bytes"),
        "memory_bytes_seen_by_installer": checks_seen.get("memory_bytes"),
    }


def last_boot(journal: str) -> tuple[str, int]:
    """The last boot in `journalctl` text output, and how many boots it holds."""
    sections = re.split(r"^-- Boot [0-9a-f]+ --$", journal, flags=re.M)
    sections = [section for section in sections if section.strip()]
    return (sections[-1] if sections else "", len(sections))


def check_first_start(name: str, label: str, out: pathlib.Path, checks: Checks, required: bool = True) -> dict:
    directory = out / name / "first-start"
    result = load(directory / "first-start.json")
    journal_path = directory / "journal-disk.txt"
    journal = journal_path.read_text(errors="replace") if journal_path.exists() else ""
    boot, boots = last_boot(journal)

    def add(ident: str, description: str, passed, detail: str, need: bool = required) -> None:
        checks.add(f"{name}.first_start.{ident}", f"{label}: {description}", passed, detail, need)

    shot = ((result or {}).get("screenshots") or {}).get("first-start") or {}
    add(
        "screen",
        "started from the installed disk alone (no medium, no network device), the display shows a rendered "
        "graphical screen that stays up",
        (
            result.get("outcome") == "rendered"
            and shot.get("looks_rendered") is True
            and result.get("still_running_after_run_on") is True
            and no_network(result.get("command")) is True
            and "id=medium" not in " ".join(result.get("command") or [])
        )
        if result
        else None,
        f"outcome {(result or {}).get('outcome')}; rendered since {(result or {}).get('rendered_first_seen_seconds')} s; "
        f"{shot.get('width')}x{shot.get('height')}, lit fraction {shot.get('lit_fraction')}, "
        f"{shot.get('distinct_colours')} colours; no network device: {no_network((result or {}).get('command'))}",
    )
    after = ((result or {}).get("screenshots") or {}).get("first-start-after-input") or {}
    add(
        "responds",
        "the display changes after a key press and pointer movement",
        (after.get("changed_fraction") or 0) > 0.001 if after else None,
        f"share of sampled pixels that changed: {after.get('changed_fraction')}",
        False,
    )
    cmdline = re.search(r"kernel: Kernel command line: (.*)$", boot, re.M)
    line = cmdline.group(1) if cmdline else ""
    add(
        "kernel_arguments",
        "the journal on the disk shows GRUB expanded the boot-counting variable: tur.boot_contract=1, no trial",
        ("tur.boot_contract=1" in line.split() and "$" not in line and "tur.boot=" not in line) if journal else None,
        line or "no kernel command line in the journal read from the disk",
    )
    graphical = re.search(r"systemd\[1\]: Reached target graphical\.target", boot)
    manager = re.search(r"systemd\[1\]: Started plasmalogin\.service", boot)
    add(
        "graphical_target",
        "the journal on the disk shows the display manager started and graphical.target was reached",
        bool(graphical and manager) if journal else None,
        f"graphical.target: {bool(graphical)}; plasmalogin.service started: {bool(manager)}; "
        f"boots in the journal: {boots}; {len(journal.splitlines())} lines",
    )
    gate = re.search(r"tur-boot-health: gate: Boot health gate (\w+)", boot)
    complete = re.search(r"systemd\[1\]: Reached target boot-complete\.target", boot)
    marked = re.search(r"tur-boot-health: mark-good: (.*)$", boot, re.M)
    add(
        "health_gate",
        "the journal on the disk shows the boot health gate finished, boot-complete.target was reached and the "
        "boot was marked good",
        bool(gate and gate.group(1) in ("passed", "degraded") and complete and marked) if journal else None,
        f"gate: {gate.group(1) if gate else 'no verdict'}; boot-complete.target: {bool(complete)}; "
        f"mark-good: {marked.group(1) if marked else 'not seen'}",
    )
    secure = re.findall(r"secureboot: Secure boot (\w+)", boot)
    return {
        "name": name,
        "outcome": (result or {}).get("outcome"),
        "rendered_first_seen_seconds": (result or {}).get("rendered_first_seen_seconds"),
        "shutdown": (result or {}).get("shutdown"),
        "secure_boot_firmware": (result or {}).get("secure_boot_firmware"),
        "secure_boot_seen_by_kernel": secure[-1] if secure else None,
        "boots_in_journal": boots,
        "gate": gate.group(1) if gate else None,
        "kernel_command_line": line or None,
    }


def check_untouched(name: str, label: str, out: pathlib.Path, checks: Checks, kind: str) -> dict:
    result = load(out / name / "result.json")
    facts = (result or {}).get("facts") or {}
    target = facts.get("target_as_listed") or {}

    def add(ident: str, description: str, passed, detail: str, need: bool = True) -> None:
        checks.add(f"{name}.{ident}", f"{label}: {description}", passed, detail, need)

    if kind == "refuse":
        refusals = facts.get("refusals") or []
        add(
            "refused",
            "a wrong phrase and the right phrase in lower case were both refused, and Enter alone went back",
            (
                len(refusals) == 2
                and all(r.get("result") == "mismatch" and r.get("code") == "TUR-INSTALL-CONFIRM-001" for r in refusals)
                and facts.get("enter_alone") == "back"
                and int(target.get("partitions") or 0) > 0
            )
            if result
            else None,
            f"disk as listed: {target}; attempts: {refusals}; Enter alone: {facts.get('enter_alone')}",
        )
    else:
        add(
            "refused",
            "a disk smaller than the minimum is shown as too small with its error code, cannot be chosen, and the "
            "installer says no disk can be installed to",
            (
                target.get("status") == "too-small"
                and target.get("code") == "TUR-INSTALL-PREFLIGHT-006"
                and target.get("index") == "-"
                and facts.get("no_target_code") == "TUR-INSTALL-MEDIUM-003"
                and facts.get("choosing_anyway") == "invalid"
            )
            if result
            else None,
            f"disk as listed: {target}; message code: {facts.get('no_target_code')}; typing a number anyway: "
            f"{facts.get('choosing_anyway')}",
        )
        add(
            "command_line",
            "the command line opens, runs a command and returns to the installer",
            (facts.get("shell_command_ran") is True and facts.get("shell_left") is True) if result else None,
            f"command ran: {facts.get('shell_command_ran')}; left: {facts.get('shell_left')}",
        )
    add(
        "disk_untouched",
        "the disk is bit-for-bit what it was before the machine started, and no installation was begun",
        (
            facts.get("disk_unchanged") is True
            and facts.get("installing_seen") is False
            and result.get("outcome") == "completed"
        )
        if result
        else None,
        f"SHA-256 before {facts.get('disk_sha256_before')}, after {facts.get('disk_sha256_after')}; "
        f"installation begun: {facts.get('installing_seen')}; outcome {(result or {}).get('outcome')}",
    )
    return {
        "name": name,
        "outcome": (result or {}).get("outcome"),
        "medium_attached_as": (result or {}).get("medium_attached_as"),
        "elapsed_seconds": (result or {}).get("elapsed_seconds"),
    }


def entry_named(entries: dict | None, description: str) -> dict | None:
    for number, entry in ((entries or {}).get("entries") or {}).items():
        if entry.get("description") == description:
            return dict(entry, number=number)
    return None


def check_beside_windows(out: pathlib.Path, checks: Checks, args, manifest: dict) -> dict:
    """Installation onto a second disk beside a Windows-like disk."""
    setup = load(out / "windows-setup" / "result.json")
    run = load(out / "beside-windows" / "result.json")
    facts = (run or {}).get("facts") or {}
    label = "Beside Windows"

    def add(ident: str, description: str, passed, detail: str, required: bool = True) -> None:
        checks.add(f"beside-windows.{ident}", f"{label}: {description}", passed, detail, required)

    created = entry_named(((setup or {}).get("firmware_boot_entries") or {}).get("after"), "Windows Boot Manager")
    add(
        "fixture",
        "the firmware holds a \"Windows Boot Manager\" entry pointing at the Windows-like disk, made with efibootmgr "
        "before the installation (the fixture)",
        (((setup or {}).get("facts") or {}).get("efibootmgr_status") == 0 and bool(created)
         and "\\EFI\\Microsoft\\Boot\\bootmgfw.efi" in (created or {}).get("device_path", ""))
        if setup else None,
        f"efibootmgr status {((setup or {}).get('facts') or {}).get('efibootmgr_status')}; entry {created}",
    )
    windows = facts.get("windows_disk_as_listed") or {}
    holds = dict(item.split("=", 1) for item in (windows.get("holds") or "").split(",") if "=" in item)
    add(
        "listed_as_windows",
        "the disk list describes the Windows-like disk as Windows, with its NTFS and BitLocker volumes",
        (holds.get("windows") == "yes" and holds.get("bitlocker") == "1" and holds.get("ntfs") == "1") if run else None,
        f"as listed: {windows}",
    )
    target = facts.get("target_as_listed") or {}
    target_holds = dict(item.split("=", 1) for item in (target.get("holds") or "").split(",") if "=" in item)
    warnings = facts.get("warnings") or {}
    add(
        "warning",
        "choosing the Windows disk shows the Windows warning before the phrase, Enter alone goes back; the blank "
        "disk shows no such warning",
        (
            bool(warnings.get("windows-disk-a"))
            and "windows=yes" in warnings.get("windows-disk-a", "")
            and facts.get("windows_disk_enter_alone") == "back"
            and "tur-boot-test" in warnings
            and warnings.get("tur-boot-test") is None
            and target_holds.get("windows") == "no"
        )
        if run
        else None,
        f"warnings by disk serial: {warnings}; Enter alone on the Windows disk: {facts.get('windows_disk_enter_alone')}",
    )
    add(
        "installed",
        "TUR was installed onto the second disk only",
        (facts.get("install_status") == 0 and facts.get("result") == "success"
         and target.get("by_id") == "/dev/disk/by-id/virtio-tur-boot-test"
         and run.get("outcome") == "completed")
        if run
        else None,
        f"bootc exit status {facts.get('install_status')} after {facts.get('install_seconds')} s; device "
        f"{facts.get('install_device')}; chosen {target.get('by_id')}",
    )
    hashes = {}
    text = out / "beside-windows" / "disk-a.sha256.env"
    if text.exists():
        for line in text.read_text().splitlines():
            key, _, value = line.partition("=")
            hashes[key.strip()] = value.strip()
    add(
        "disk_a_identical",
        "the Windows-like disk is byte-identical after the setup, the installation and TUR's first start "
        "(SHA-256 of the whole image)",
        (bool(hashes.get("created")) and hashes.get("created") == hashes.get("after_install")
         == hashes.get("after_first_start"))
        if hashes else None,
        "; ".join(f"{key} {value}" for key, value in hashes.items()) or "no hashes recorded",
    )
    install_after = ((run or {}).get("firmware_boot_entries") or {}).get("after")
    first = load(out / "beside-windows" / "first-start" / "first-start.json")
    first_after = ((first or {}).get("firmware_boot_entries") or {}).get("after")
    kept_install = entry_named(install_after, "Windows Boot Manager")
    kept_first = entry_named(first_after, "Windows Boot Manager")
    new_install = [
        entry for entry in ((install_after or {}).get("entries") or {}).values()
        if "fedora" in (entry.get("description") or "").lower() or "\\fedora\\" in (entry.get("device_path") or "").lower()
    ]
    add(
        "windows_entry_kept",
        "the \"Windows Boot Manager\" entry is unchanged after the installation and after TUR's first start, and "
        "the installation itself added no boot entry",
        (
            bool(created and kept_install and kept_first)
            and kept_install.get("device_path") == created.get("device_path")
            and kept_first.get("device_path") == created.get("device_path")
            and not new_install
        )
        if (run and first)
        else None,
        f"after the installation: {kept_install}; entries naming Fedora after it: {new_install}; "
        f"after the first start: {kept_first}",
    )
    order_before = (install_after or {}).get("boot_order_descriptions")
    order_after = (first_after or {}).get("boot_order_descriptions")
    add(
        "boot_order",
        "what the first start of TUR did to the firmware's boot order (recorded; shim's fallback adds its own entry)",
        True if (install_after and first_after) else None,
        f"after the installation: {order_before}; after TUR's first start: {order_after}",
        False,
    )
    start = check_first_start("beside-windows", "Beside Windows, first start of the second disk", out, checks)
    check_layout("beside-windows", label, out / "beside-windows", checks, args,
                 (manifest.get("payload") or {}).get("recorded_origin"))
    return {
        "windows_disk_as_listed": windows,
        "warnings": warnings,
        "disk_a_sha256": hashes,
        "boot_order_after_install": order_before,
        "boot_order_after_first_start": order_after,
        "first_start": start,
    }


def check_live(name: str, label: str, out: pathlib.Path, checks: Checks, windows: bool, required: bool = True) -> dict:
    """The live session and the graphical installer (tests/iso/live.py)."""
    directory = out / name
    result = load(directory / "result.json")
    facts = (result or {}).get("facts") or {}
    live = facts.get("live") or {}
    root = load(directory / "installed-root" / "installed-root.json")

    def add(ident: str, description: str, passed, detail: str, need: bool = required) -> None:
        checks.add(f"{name}.live.{ident}", f"{label}: {description}", passed, detail, need)

    shots = {shot.get("screen"): shot for shot in (result or {}).get("screenshots") or []}
    desktop = shots.get("live-desktop") or {}
    cmdline = facts.get("kernel_command_line") or ""
    add(
        "session",
        "the medium's default boot entry started the live session, signed in by itself, and the graphical "
        "installer opened on the desktop",
        (
            "tur.live=1" in cmdline.split()
            and "tur.installer=1" not in cmdline.split()
            and live.get("wizard_started_seconds") is not None
            and (live.get("wizard_start") or "").find("started_by_session=yes") >= 0
            and desktop.get("looks_rendered") is True
        )
        if result
        else None,
        f"kernel command line: {cmdline}; the session's serial service after {live.get('session_service_seconds')} s; "
        f"installer started after {live.get('wizard_started_seconds')} s ({live.get('wizard_start')}); window "
        f"{live.get('window')}, active: {live.get('window_active')}; desktop screenshot {desktop}",
    )
    add(
        "keyboard_only",
        "the installer was driven with the keyboard alone (QMP send-key: Tab, Space, letters), as a person can",
        (result.get("outcome") == "completed" and "send-key" in (result.get("input") or "")) if result else None,
        f"outcome {(result or {}).get('outcome')}; {(result or {}).get('input')}; "
        f"{len((result or {}).get('steps') or [])} steps",
    )
    medium = live.get("medium_as_listed") or {}
    add(
        "medium_refused",
        "the medium the installer runs from is listed as such, cannot be chosen, and Tab never reaches it",
        (medium.get("status") == "installer-medium" and live.get("medium_reached_by_tab") is False) if result else None,
        f"medium as listed: {medium}; Tab passed: {live.get('tab_stops_on_the_disk_screen')}",
    )
    typed = live.get("phrase_typed") or {}
    add(
        "phrase",
        "the erase button is enabled by the exact phrase only, and Enter in the field does not erase",
        (typed.get("matches") == "yes" and typed.get("erase_enabled") == "yes" and live.get("enter_erased") is False
         and live.get("focus_on_confirm") == "phraseField")
        if result
        else None,
        f"after typing the phrase: {typed}; Enter erased: {live.get('enter_erased')}; keyboard on opening the "
        f"confirmation: {live.get('focus_on_confirm')}",
    )
    if windows:
        attempts = live.get("windows_phrase_attempts") or []
        windows_listed = live.get("windows_disk_as_listed") or {}
        confirm_windows = live.get("confirm_windows") or {}
        confirm_blank = live.get("confirm_blank") or {}
        add(
            "windows_warning",
            "the Windows-like disk is described as Windows with BitLocker; choosing it shows the Windows warning; "
            "the blank disk shows none",
            (windows_listed.get("windows") == "yes" and windows_listed.get("bitlocker") == "yes"
             and confirm_windows.get("windows_warning") == "yes" and confirm_blank.get("windows_warning") == "no")
            if result
            else None,
            f"listed: {windows_listed}; confirmation of the Windows disk: {confirm_windows}; of the blank disk: "
            f"{confirm_blank}",
        )
        add(
            "phrase_required_for_data",
            "on the disk that holds data, a wrong phrase and the right phrase in lower case leave the erase button "
            "unavailable, and nothing is asked of the engine",
            (len(attempts) == 2 and all(item.get("erase_enabled") == "no" for item in attempts)
             and live.get("windows_erase_requested") is False)
            if result
            else None,
            f"attempts: {attempts}; erase asked: {live.get('windows_erase_requested')}",
        )
        hashes = {}
        text = directory / "disk-a.sha256.env"
        if text.exists():
            for line in text.read_text().splitlines():
                key, _, value = line.partition("=")
                hashes[key.strip()] = value.strip()
        add(
            "windows_disk_identical",
            "the Windows-like disk is byte-identical after the installation and after TUR's first start "
            "(SHA-256 of the whole image)",
            (bool(hashes.get("created")) and hashes.get("created") == hashes.get("after_install")
             == hashes.get("after_first_start"))
            if hashes
            else None,
            "; ".join(f"{key} {value}" for key, value in hashes.items()) or "no hashes recorded",
        )
    stages = live.get("stages_seen") or []
    expected = ["revalidate", "prepare", "copy", "bootloader", "finish", "check", "record"]
    add(
        "stages",
        "the progress screen followed the engine's real stages, in order, from bootc's own output",
        ([stage for stage in stages if stage in expected] == expected) if result else None,
        f"stages seen: {stages}",
    )
    add(
        "cancel",
        "Cancel was offered while the engine had written nothing, and not once the disk was being written",
        (live.get("cancellable_while_revalidating") == "yes" and live.get("cancellable_while_copying") == "no")
        if result
        else None,
        f"while looking at the disk again: {live.get('cancellable_while_revalidating')}; while copying: "
        f"{live.get('cancellable_while_copying')}",
    )
    kept = facts.get("log_kept") or {}
    files = (root or {}).get("installer_log_files") or []
    add(
        "log_kept",
        "the installer's log is kept on the installed disk, in /var/log/tur-installer/<session>, with the SELinux "
        "label of /var/log",
        (kept.get("result") == "yes" and len(files) >= 2 and bool((root or {}).get("installer_log_directory_selinux"))
         and (root or {}).get("engine_lines_in_kept_log", 0) > 0)
        if (result and root)
        else None,
        f"engine: {kept}; on the disk: {(root or {}).get('installer_log_directories')}, files "
        f"{[(item.get('name'), item.get('bytes'), item.get('selinux')) for item in files]}, directory label "
        f"{(root or {}).get('installer_log_directory_selinux')}; lines from the engine in it: "
        f"{(root or {}).get('engine_lines_in_kept_log')}",
    )
    add(
        "root_features",
        "the installed root filesystem's features are recorded; casefold is not among them (GRUB reads this "
        "filesystem and does not know casefold)",
        ((root or {}).get("casefold") is False and bool((root or {}).get("features"))) if root else None,
        f"features: {(root or {}).get('features')}; engine: {facts.get('root_filesystem')}",
        False,
    )
    return {
        "name": name,
        "session_service_seconds": live.get("session_service_seconds"),
        "wizard_started_seconds": live.get("wizard_started_seconds"),
        "welcome_seconds": live.get("welcome_seconds"),
        "window_active": live.get("window_active"),
        "stages_seen": stages,
        "root_features": (root or {}).get("features"),
        "log_on_disk": (root or {}).get("installer_log_directories"),
    }


def check_casefold(out: pathlib.Path, checks: Checks) -> dict:
    """Informational: the installed disk given ext4's casefold feature, started."""
    facts = load(out / "casefold" / "casefold.json")
    start = load(out / "casefold" / "start" / "first-start.json")
    checks.add(
        "casefold.experiment",
        "Informational (issue 81): the installed root given ext4's casefold feature afterwards (tune2fs, "
        "unmounted), then started: whether a rendered screen appears",
        (facts.get("casefold") is True and start is not None) if facts else None,
        f"tune2fs: {(facts or {}).get('tune2fs')}; e2fsck after: status {((facts or {}).get('e2fsck_after') or {}).get('status')}; "
        f"start: outcome {(start or {}).get('outcome')}, rendered after {(start or {}).get('rendered_first_seen_seconds')} s",
        False,
    )
    return {"casefold": facts, "start_outcome": (start or {}).get("outcome")}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--job", required=True, choices=["product", "proof"])
    ap.add_argument("--out", required=True)
    ap.add_argument("--expected-build-id", default="")
    ap.add_argument("--timing", action="append", default=[], help="NAME=SECONDS, recorded as given")
    args = ap.parse_args()
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    checks = Checks()
    manifest = check_medium(out, checks, args)
    installs, starts, refusals = [], [], []
    extra: dict = {}

    if args.job == "product":
        refusals.append(check_untouched("too-small", "Disk too small", out, checks, "too-small"))
        refusals.append(check_untouched("refuse", "Disk with data, not confirmed", out, checks, "refuse"))
        installs.append(check_install("install", "Blank disk, medium as a USB stick", out, checks, args, manifest))
        starts.append(check_first_start("install", "Blank disk, first start", out, checks))
        installs.append(
            check_install("install-over-data", "Disk with data, confirmed, medium as an optical drive", out, checks, args, manifest)
        )
        starts.append(check_first_start("install-over-data", "Disk with data, first start", out, checks))
        extra["beside_windows"] = check_beside_windows(out, checks, args, manifest)
        # The live session and the graphical installer, beside the
        # Windows-like disk (ADR-0018, amendment of 13 November 2026).
        installs.append(check_install("live", "Live session, graphical installer, beside Windows", out, checks, args, manifest))
        extra["live"] = check_live("live", "Live session, graphical installer", out, checks, windows=True)
        starts.append(check_first_start("live", "Live session, first start (Windows-like disk attached)", out, checks))
        extra["casefold"] = check_casefold(out, checks)
        # Secure Boot: recorded, never required. Nothing in TUR has been
        # verified by firmware before this.
        installs.append(
            check_install("secure-boot", "Secure Boot enforced (informational)", out, checks, args, manifest, required=False)
        )
        secure = load(out / "secure-boot" / "result.json")
        seen = (((secure or {}).get("facts") or {}).get("checks") or {}).get("secure_boot")
        checks.add(
            "secure-boot.enforced",
            "Secure Boot enforced (informational): the installer environment, started through shim and GRUB, "
            "reports Secure Boot enabled",
            (seen == "enabled") if secure else None,
            f"firmware {(secure or {}).get('firmware')}; the installer reports Secure Boot {seen}",
            False,
        )
        start = check_first_start("secure-boot", "Secure Boot enforced, first start (informational)", out, checks, required=False)
        starts.append(start)
        checks.add(
            "secure-boot.first_start.enforced",
            "Secure Boot enforced, first start (informational): the installed system's kernel reports Secure Boot enabled",
            (start.get("secure_boot_seen_by_kernel") == "enabled") if start.get("outcome") else None,
            f"kernel: Secure boot {start.get('secure_boot_seen_by_kernel')}",
            False,
        )
    else:
        installs.append(check_install("install", "Boot-test image, live session, graphical installer, medium as a "
                                      "USB stick", out, checks, args, manifest))
        live = check_live("install", "Boot-test image, live session", out, checks, windows=False)
        comparison = load(out / "layout-comparison.json")
        checks.add(
            "layout.same_as_boot_test",
            "The disk installed from the medium has the same layout as the disk tests/boot/build-disk.sh installs "
            "from the same image: partition table, EFI system partition files, GRUB configuration, kernel arguments",
            comparison.get("identical") if comparison else None,
            f"compared: {', '.join((comparison or {}).get('compared') or [])}; differences: "
            f"{[item.get('what') for item in (comparison or {}).get('differences') or []]}",
        )
        boot = load(out / "boot" / "boot-report.json")
        items = (boot or {}).get("checks") or []
        failed = [item["id"] for item in items if item["required"] and item["result"] != "pass"]
        checks.add(
            "boot.checks",
            "The boot test's own checks pass on the disk installed from the medium: three boots to a graphical "
            "session, boot counting, the health gate",
            (boot.get("result") == "pass" and boot.get("build_id") == args.expected_build_id) if boot else None,
            f"tests/boot/report.py: {(boot or {}).get('result')}; {len(items)} checks, "
            f"{sum(1 for item in items if item['required'])} required; not passed: {failed}; build ID {(boot or {}).get('build_id')}",
        )
        rollback = load(out / "rollback" / "rollback-report.json")
        scenarios = (rollback or {}).get("scenarios") or []
        checks.add(
            "rollback.scenarios",
            "A deliberately failing update staged on the disk installed from the medium is undone, and a good one "
            "is kept (tests/boot/rollback.py)",
            (rollback.get("result") == "pass" and len(scenarios) >= 1) if rollback else None,
            "; ".join(
                f"{item.get('id')} {item.get('scenario')}: {item.get('result')} ({', '.join(item.get('sequence_after_staging') or [])})"
                for item in scenarios
            )
            or "no rollback report",
        )
        extra = {"layout_comparison": comparison, "boot_report_result": (boot or {}).get("result"), "rollback": rollback,
                 "live": live}

    timings = {}
    for item in args.timing:
        name, _, value = item.partition("=")
        try:
            timings[name] = float(value)
        except ValueError:
            timings[name] = value or None
    medium = manifest.get("medium") or {}
    report = {
        "schema_version": 1,
        "task": "TUR-007, TUR-009",
        "job": args.job,
        "result": "pass" if checks.passed() else "fail",
        "expected_build_id": args.expected_build_id or None,
        "medium": {
            "file": medium.get("file"),
            "bytes": medium.get("bytes"),
            "sha256": medium.get("sha256"),
            "payload_image_id": (manifest.get("payload") or {}).get("image_id"),
            "payload_bytes_on_medium": ((manifest.get("payload") or {}).get("oci_layout") or {}).get("bytes"),
            "installer_environment_bytes": (manifest.get("installer_environment") or {}).get("squashfs_bytes"),
            "build_seconds": manifest.get("build_seconds"),
        },
        "installations": installs,
        "first_starts": starts,
        "refusals": refusals,
        "timings_seconds": timings,
        "checks": checks.items,
        **extra,
    }
    (out / "iso-report.json").write_text(json.dumps(report, indent=2) + "\n")

    icon = {"pass": "pass", "fail": "**FAIL**", "not-run": "**NOT RUN**"}
    lines = [
        f"### TUR installer medium, {args.job}: {report['result'].upper()}",
        "",
        "| Check | Result | Evidence |",
        "|---|---|---|",
    ]
    for item in checks.items:
        name = item["description"] + ("" if item["required"] else " (not required)")
        lines.append(f"| {name} | {icon[item['result']]} | {item['detail'].replace('|', '/')[:900]} |")
    size = medium.get("bytes")
    facts = [
        ("Medium", f"{medium.get('file')}, {size} bytes ({round(size / 2**30, 2) if size else '?'} GiB), sha256 {medium.get('sha256')}"),
        ("Build of the medium (s)", json.dumps(manifest.get("build_seconds"))),
        ("Installations", "; ".join(
            f"{i['name']}: installer up after {i['installer_started_seconds']} s, first screen ready after "
            f"{i['welcome_seconds']} s, bootc install {i['install_seconds']} s, whole run {i['elapsed_seconds']} s"
            for i in installs
        )),
        ("First starts", "; ".join(
            f"{s['name']}: rendered after {s['rendered_first_seen_seconds']} s, gate {s['gate']}, Secure Boot seen by "
            f"the kernel: {s['secure_boot_seen_by_kernel']}, power-off {s['shutdown']}"
            for s in starts
        )),
        ("Timings (s)", ", ".join(f"{k}={v}" for k, v in timings.items())),
    ]
    lines += ["", "| | |", "|---|---|"]
    lines += [f"| {key} | `{str(value).replace('|', '/')}` |" for key, value in facts]
    lines += [
        "",
        "All of this ran in QEMU/KVM with OVMF. It says nothing about real firmware, real disks,",
        "real USB sticks, graphics hardware or wireless-only machines.",
        "",
    ]
    summary = "\n".join(lines)
    (out / "summary.md").write_text(summary)
    print(summary)
    return 0 if checks.passed() else 1


if __name__ == "__main__":
    sys.exit(main())
