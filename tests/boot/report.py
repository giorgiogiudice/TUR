#!/usr/bin/env python3
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
"""Turn the evidence of a boot test into boot-report.json and a summary.

    tests/boot/report.py --work WORK_DIR --out OUT_DIR --expected-build-id ID
                         [--expected-failed-units FILE] [--timing NAME=SECONDS ...]

Evidence, all optional (a missing piece fails the checks that need it):

    WORK_DIR/report.tar            facts sent by the guest agent
    OUT_DIR/guest-from-disk/       the same facts recovered from the disk
    OUT_DIR/serial.log             serial console
    OUT_DIR/screenshot.png         virtual display
    OUT_DIR/host/vm-result.json    what tests/boot/run-vm.py saw
    OUT_DIR/host/disk-layout.json  what tests/boot/inspect-disk.py measured
    OUT_DIR/host/disk-info.env     what tests/boot/build-disk.sh did
    OUT_DIR/host/kvm.json          tests/boot/check-kvm.py

Writes OUT_DIR/boot-report.json, OUT_DIR/summary.md, OUT_DIR/journal.txt and
OUT_DIR/journal.export.gz, prints the summary, and exits 0 only if every
required check passed. A check is never downgraded to make a run pass: one that
could not be evaluated is reported as "not-run" and counts as a failure when
required.
"""

from __future__ import annotations

import argparse
import io
import json
import pathlib
import re
import shutil
import sys
import tarfile

MARK = "UOS-BOOT-TEST: "
ESP_GUID = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"


def props(text: str) -> dict[str, str]:
    """KEY=VALUE lines as a dictionary."""
    found = {}
    for line in text.splitlines():
        key, sep, value = line.partition("=")
        if sep:
            found[key.strip()] = value.strip()
    return found


def span_seconds(text: str) -> float | None:
    """A systemd time span such as '1min 2.345s' or '812ms' in seconds."""
    units = {"min": 60.0, "s": 1.0, "ms": 0.001, "us": 0.000001, "h": 3600.0}
    total, matched = 0.0, False
    for number, unit in re.findall(r"([0-9.]+)\s*(min|ms|us|s|h)\b", text):
        total += float(number) * units[unit]
        matched = True
    return round(total, 3) if matched else None


class Evidence:
    def __init__(self, work: pathlib.Path, out: pathlib.Path):
        self.out = out
        self.guest = out / "guest"
        self.guest_source = "none"
        tar_path = work / "report.tar"
        if tar_path.exists() and tar_path.stat().st_size > 0:
            try:
                self.guest.mkdir(parents=True, exist_ok=True)
                # The stream may carry padding after the archive; read it as a stream.
                with tarfile.open(fileobj=io.BytesIO(tar_path.read_bytes()), mode="r|") as archive:
                    archive.extractall(self.guest, filter="data")
                self.guest_source = "virtio-serial"
            except (tarfile.TarError, OSError, EOFError) as error:
                print(f"report.tar could not be read completely: {error}")
                if (self.guest / "status").exists():
                    self.guest_source = "virtio-serial (truncated)"
        if not (self.guest / "status").exists() and (out / "guest-from-disk" / "status").exists():
            shutil.copytree(out / "guest-from-disk", self.guest, dirs_exist_ok=True)
            self.guest_source = "disk (post-mortem)"
        self.status = props(self.text("status"))
        self.vm = self.json(out / "host" / "vm-result.json") or {}
        self.layout = self.json(out / "host" / "disk-layout.json") or {}
        self.kvm = self.json(out / "host" / "kvm.json") or {}
        info = out / "host" / "disk-info.env"
        self.disk_info = props(info.read_text()) if info.exists() else {}
        serial = out / "serial.log"
        self.serial = serial.read_bytes().decode("utf-8", errors="replace") if serial.exists() else ""

    def text(self, name: str) -> str:
        path = self.guest / name
        try:
            return path.read_bytes().decode("utf-8", errors="replace")
        except OSError:
            return ""

    def line(self, name: str) -> str:
        text = self.text(name).strip()
        return text.splitlines()[0].strip() if text else ""

    def json(self, path: pathlib.Path | str):
        if isinstance(path, str):
            path = self.guest / path
        try:
            return json.loads(path.read_bytes().decode("utf-8", errors="replace"))
        except (OSError, ValueError):
            return None


class Checks:
    def __init__(self):
        self.items: list[dict] = []

    def add(self, ident: str, description: str, passed: bool | None, detail: str, required: bool = True) -> None:
        result = "not-run" if passed is None else "pass" if passed else "fail"
        self.items.append(
            {"id": ident, "description": description, "required": required, "result": result, "detail": detail}
        )

    def passed(self) -> bool:
        return all(item["result"] == "pass" for item in self.items if item["required"])


def expected_failures(path: pathlib.Path | None) -> dict[str, str]:
    found = {}
    if path and path.exists():
        for line in path.read_text().splitlines():
            line = line.split("#", 1)[0].strip()
            if line:
                unit, _, reason = line.partition(" ")
                found[unit] = reason.strip()
    return found


def boot_loader(ev: Evidence) -> dict:
    """What booted the system, from firmware variables, the ESP and bootupd."""
    efibootmgr = ev.text("efibootmgr.txt")
    current = re.search(r"^BootCurrent:\s*([0-9A-Fa-f]{4})", efibootmgr, re.M)
    entry = None
    if current:
        match = re.search(rf"^Boot{current.group(1)}\*?\s+(.*)$", efibootmgr, re.M)
        entry = " ".join(match.group(1).split()) if match else None
    efi_path = None
    if entry:
        match = re.search(r"\\EFI\\[^\s)]+", entry)
        efi_path = match.group(0) if match else None
    bootctl = ev.text("bootctl-status.txt")
    product = re.search(r"Current Boot Loader:\s*\n\s*Product:\s*(.+)", bootctl)
    firmware = re.search(r"^\s*Firmware:\s*(.+)$", bootctl, re.M)
    secure = re.search(r"^\s*Secure Boot:\s*(.+)$", bootctl, re.M)
    esp_files = [name.lower() for name in ev.layout.get("esp_files", [])]
    has_shim = any(name.endswith("/shimx64.efi") for name in esp_files)
    has_grub = any(name.endswith("/grubx64.efi") for name in esp_files)
    has_sdboot = any("systemd-boot" in name for name in esp_files)
    loader_info = ev.status.get("loader_info", "") or (product.group(1).strip() if product else "")
    grub_on_serial = bool(re.search(r"GRUB|grub", ev.serial[:200_000]))
    if "grub" in loader_info.lower() or (has_grub and not has_sdboot):
        name = "GRUB 2"
        if has_shim and (efi_path is None or "shim" in efi_path.lower() or "boot" in efi_path.lower()):
            name += ", loaded by shim"
    elif "systemd-boot" in loader_info.lower() or has_sdboot:
        name = "systemd-boot"
    else:
        name = None
    return {
        "in_use": name,
        "loader_info": loader_info or None,
        "firmware": firmware.group(1).strip() if firmware else None,
        "secure_boot": (secure.group(1).strip() if secure else None) or ev.line("secure-boot.txt") or None,
        "efi_boot_current": current.group(1) if current else None,
        "efi_boot_entry": entry,
        "efi_binary": efi_path,
        "esp_has_shim": has_shim,
        "esp_has_grub": has_grub,
        "esp_has_systemd_boot": has_sdboot,
        "grub_seen_on_serial_console": grub_on_serial,
        "boot_loader_entries": ev.layout.get("boot_entries"),
        "bootupd_status": ev.text("bootupctl-status.txt").strip() or None,
        "kernel_command_line": ev.line("cmdline.txt") or None,
    }


def mount_of(ev: Evidence, name: str) -> dict | None:
    data = ev.json(name)
    try:
        fs = data["filesystems"][0]
        return {"source": fs.get("source"), "fstype": fs.get("fstype"), "options": fs.get("options")}
    except (TypeError, KeyError, IndexError):
        return None


def guest_block_devices(ev: Evidence) -> list[dict]:
    data = ev.json("lsblk.json") or {}
    devices = []
    for device in data.get("blockdevices", []):
        if device.get("type") != "disk":
            continue
        devices.append(
            {
                "name": device.get("name"),
                "size_bytes": device.get("size"),
                "serial": device.get("serial"),
                "partitions": [
                    {
                        "name": child.get("name"),
                        "size_bytes": child.get("size"),
                        "partlabel": child.get("partlabel"),
                        "parttype": child.get("parttype"),
                        "parttypename": child.get("parttypename"),
                        "fstype": child.get("fstype"),
                        "mountpoints": [m for m in (child.get("mountpoints") or []) if m],
                    }
                    for child in device.get("children") or []
                ],
            }
        )
    return devices


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--work", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--expected-build-id", default="")
    ap.add_argument("--expected-failed-units")
    ap.add_argument("--timing", action="append", default=[], help="NAME=SECONDS, recorded as given")
    args = ap.parse_args()

    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    ev = Evidence(pathlib.Path(args.work), out)
    for name in ("journal.txt", "journal.export.gz"):
        if (ev.guest / name).exists():
            shutil.move(ev.guest / name, out / name)
    st, vm = ev.status, ev.vm
    checks = Checks()

    # ---- virtual machine ---------------------------------------------------------
    kvm_enabled = (vm.get("kvm") or {}).get("enabled")
    virt = ev.line("virt.txt")
    checks.add(
        "kvm",
        "The guest ran under KVM acceleration, not software emulation",
        (bool(kvm_enabled) and virt == "kvm") if vm else None,
        f"QEMU query-kvm enabled={kvm_enabled}; guest systemd-detect-virt={virt or 'no data'}",
    )
    checks.add(
        "uefi",
        "The guest booted through UEFI firmware",
        st.get("uefi") == "yes" if st else None,
        f"/sys/firmware/efi present: {st.get('uefi', 'no data')}; firmware image {vm.get('firmware')}",
    )
    kernel_on_serial = re.search(r"Linux version (\S+)", ev.serial)
    checks.add(
        "serial_console",
        "The serial console was captured from firmware to the test agent",
        bool(ev.serial) and bool(kernel_on_serial) and (MARK + "START") in ev.serial,
        f"{len(ev.serial)} bytes; kernel banner {'seen' if kernel_on_serial else 'missing'}; "
        f"agent marker {'seen' if (MARK + 'START') in ev.serial else 'missing'}",
    )

    # ---- identity ------------------------------------------------------------------
    build_id = st.get("build_id", "")
    checks.add(
        "image_identity",
        "The booted system is the image built from this commit",
        (build_id == args.expected_build_id and bool(build_id)) if st else None,
        f"booted BUILD_ID={build_id or 'no data'}; expected {args.expected_build_id or 'not given'}",
    )
    kernel = st.get("kernel", "")
    checks.add(
        "kernel",
        "The running kernel is the one shipped in the image",
        (bool(kernel) and st.get("kernel_modules_match") == "yes") if st else None,
        f"uname -r={kernel or 'no data'}; /usr/lib/modules/<release> present: {st.get('kernel_modules_match', 'no data')}",
    )

    # ---- graphical target and display manager ---------------------------------------
    graphical = props(ev.text("unit-graphical.txt"))
    graphical_us = int(graphical.get("ActiveEnterTimestampMonotonic", "0") or 0)
    checks.add(
        "graphical_target",
        "systemd reached graphical.target",
        (st.get("graphical_target") == "reached" and graphical.get("ActiveState") == "active") if st else None,
        f"agent wait: {st.get('graphical_target', 'no data')}; ActiveState={graphical.get('ActiveState', 'no data')}",
    )
    dm = props(ev.text("unit-dm.txt"))
    alias = props(ev.text("unit-dm-alias.txt"))
    failure = props(ev.text("unit-dm-failure.txt"))
    failure_ran = failure.get("ExecMainStartTimestampMonotonic", "0") not in ("0", "")
    dm_ok = (
        dm.get("ActiveState") == "active"
        and dm.get("SubState") == "running"
        and dm.get("NRestarts") == "0"
        and alias.get("Id") == dm.get("Id")
        and st.get("display_manager_after_hold") == "active"
        and not failure_ran
    )
    checks.add(
        "display_manager",
        "The display manager started and stayed up",
        dm_ok if dm else None,
        f"{dm.get('Id', 'no data')}: {dm.get('ActiveState', '?')}/{dm.get('SubState', '?')}, "
        f"restarts={dm.get('NRestarts', '?')}; display-manager.service is {alias.get('Id', '?')}; "
        f"failure handler ran: {failure_ran}",
    )
    first, second = st.get("greeter_pids_first", ""), st.get("greeter_pids_second", "")
    checks.add(
        "greeter",
        "The greeter's compositor and login screen processes run and survive a hold period",
        (st.get("greeter") == "up" and first == second and "none" not in first and bool(first)) if st else None,
        f"agent wait: {st.get('greeter', 'no data')}; kwin_wayland/plasma-login-greeter PIDs {first or '?'} then {second or '?'}",
    )
    shot = vm.get("screenshot") or {}
    shot_file = out / "screenshot.png"
    checks.add(
        "screenshot",
        "A screenshot of the virtual display shows rendered content, not a black or text screen",
        (bool(shot.get("looks_rendered")) and shot_file.exists() and shot.get("taken") == "after the agent reported")
        if shot
        else None,
        f"{shot.get('width', '?')}x{shot.get('height', '?')}, lit fraction {shot.get('lit_fraction', '?')}, "
        f"{shot.get('distinct_colours', '?')} colours sampled, taken {shot.get('taken', 'never')}",
    )

    # ---- network ---------------------------------------------------------------------
    addresses = []
    for link in ev.json("ip-addr.json") or []:
        if link.get("ifname") == "lo":
            continue
        for addr in link.get("addr_info", []):
            if addr.get("family") == "inet" and addr.get("scope") == "global":
                addresses.append(f"{link.get('ifname')} {addr.get('local')}/{addr.get('prefixlen')}")
    checks.add(
        "network",
        "The guest has an IPv4 address, a default route and a reachable gateway",
        (bool(addresses) and st.get("default_route") == "yes" and st.get("gateway_ping") == "ok") if st else None,
        f"addresses: {', '.join(addresses) or 'none'}; default route: {st.get('default_route', 'no data')}; "
        f"gateway {st.get('gateway', '?')} ping: {st.get('gateway_ping', 'no data')}",
    )

    # ---- storage and bootc -------------------------------------------------------------
    devices = guest_block_devices(ev)
    sysroot = mount_of(ev, "findmnt-sysroot.json")
    root = mount_of(ev, "findmnt-root.json")
    guest_parts = [part for device in devices for part in device["partitions"]]
    has_esp = any((part.get("parttype") or "").lower() == ESP_GUID for part in guest_parts)
    sysroot_part = next((part for part in guest_parts if "/sysroot" in part["mountpoints"]), None)
    checks.add(
        "storage",
        "The disk is partitioned as installed and the physical root is mounted at /sysroot",
        (has_esp and sysroot is not None and sysroot_part is not None) if st else None,
        f"/sysroot: {sysroot}; EFI system partition seen: {has_esp}; "
        f"partitions: {', '.join(str(p.get('partlabel') or p.get('name')) for p in guest_parts) or 'no data'}",
    )
    bootc = ev.json("bootc-status.json") or {}
    booted = ((bootc.get("status") or {}).get("booted")) or {}
    booted_image = ((booted.get("image") or {}).get("image") or {}).get("image")
    test_image = ev.disk_info.get("test_image", "")
    checks.add(
        "bootc_status",
        "bootc reports a booted deployment of the installed image",
        (bool(booted_image) and (not test_image or booted_image == test_image)) if st else None,
        f"booted image: {booted_image or 'no data'}; installed from: {test_image or 'unknown'}; "
        f"host type: {(bootc.get('status') or {}).get('type', '?')}",
    )
    layout_parts = ev.layout.get("partitions", [])
    roles = [part.get("role") for part in layout_parts]
    checks.add(
        "partition_layout",
        "The installed disk has a GPT with an EFI system partition and a root partition",
        (ev.layout.get("label") == "gpt" and "esp" in roles and "root" in roles) if ev.layout else None,
        "; ".join(
            f"{p['number']}: {p.get('name')} {p.get('role')} {p.get('filesystem')} {round(p['size_bytes'] / 2**20)} MiB"
            for p in layout_parts
        )
        or "no data",
    )
    loader = boot_loader(ev)
    checks.add(
        "bootloader",
        "The bootloader that started the system is identified",
        (bool(loader["in_use"]) and bool(loader["efi_boot_entry"]) and bool(loader["boot_loader_entries"]))
        if (st or ev.layout)
        else None,
        f"{loader['in_use'] or 'not identified'}; firmware entry: {loader['efi_boot_entry'] or 'no data'}; "
        f"LoaderInfo: {loader['loader_info'] or 'not set'}; boot loader entries: {loader['boot_loader_entries']}",
    )

    # ---- health ----------------------------------------------------------------------
    failed = [unit.get("unit") for unit in (ev.json("failed-units.json") or []) if unit.get("unit")]
    allowed = expected_failures(pathlib.Path(args.expected_failed_units) if args.expected_failed_units else None)
    unexpected = [unit for unit in failed if unit not in allowed]
    have_failed_list = (ev.guest / "failed-units.json").exists() and ev.json("failed-units.json") is not None
    checks.add(
        "failed_units",
        "No systemd unit failed, other than those listed as expected in a virtual machine",
        (not unexpected) if have_failed_list else None,
        f"failed: {', '.join(failed) or 'none'}; unexpected: {', '.join(unexpected) or 'none'}",
    )
    enforce = ev.line("getenforce.txt")
    checks.add(
        "selinux",
        "SELinux is enforcing in the booted system",
        (enforce == "Enforcing") if st else None,
        f"getenforce: {enforce or 'no data'}",
    )
    journal = out / "journal.txt"
    journal_lines = sum(1 for _ in open(journal, errors="replace")) if journal.exists() else 0
    checks.add(
        "journal",
        "The journal of this boot was exported",
        journal_lines > 200 and (out / "journal.export.gz").exists(),
        f"journal.txt: {journal_lines} lines; export: "
        f"{(out / 'journal.export.gz').stat().st_size if (out / 'journal.export.gz').exists() else 0} bytes",
    )
    time_to_graphical = round(graphical_us / 1e6, 3) if graphical_us else None
    checks.add(
        "boot_time",
        "The time from kernel start to graphical.target was measured",
        time_to_graphical is not None if st else None,
        f"{time_to_graphical} s" if time_to_graphical is not None else "no data",
    )
    checks.add(
        "clean_shutdown",
        "The guest powered off when asked",
        (vm.get("shutdown") == "clean") if vm.get("shutdown") else None,
        f"shutdown: {vm.get('shutdown', 'not attempted')}",
        required=False,
    )

    # ---- report ------------------------------------------------------------------------
    analyze = ev.text("systemd-analyze-time.txt")
    phases = {}
    for value, phase in re.findall(r"((?:[0-9.]+(?:min|ms|us|s|h)\s*)+)\((firmware|loader|kernel|initrd|userspace)\)", analyze):
        phases[phase] = span_seconds(value)
    uos_release = props(ev.text("uos-release"))
    avc = [line for line in ev.text("avc-denials.txt").splitlines() if "avc:" in line]
    timings = {}
    for item in args.timing:
        name, _, value = item.partition("=")
        try:
            timings[name] = float(value)
        except ValueError:
            timings[name] = value
    if "install_seconds" in ev.disk_info:
        timings["disk_install_seconds"] = float(ev.disk_info["install_seconds"])
    timings["vm_seconds"] = vm.get("elapsed_seconds")
    dm_us = int(dm.get("ActiveEnterTimestampMonotonic", "0") or 0)
    report = {
        "schema_version": 1,
        "task": "UOS-003",
        "result": "pass" if checks.passed() else "fail",
        "build_id": build_id or None,
        "expected_build_id": args.expected_build_id or None,
        "source_revision": uos_release.get("UOS_SOURCE_REVISION"),
        "kernel": kernel or None,
        "boot": {
            "time_to_graphical_target_seconds": time_to_graphical,
            "time_to_graphical_target_note": "kernel start to graphical.target active, guest monotonic clock",
            "time_to_display_manager_seconds": round(dm_us / 1e6, 3) if dm_us else None,
            "systemd_analyze": " ".join(analyze.split()) or None,
            "phases_seconds": phases or None,
            "system_state": st.get("system_state"),
            "host_wall_clock_seconds": vm.get("milestones"),
            "firmware_resets_seen": vm.get("resets_seen"),
        },
        "failed_units": failed,
        "expected_failed_units": allowed,
        "unexpected_failed_units": unexpected,
        "bootloader": loader,
        "partition_layout": {
            "installed": ev.layout or None,
            "install_command": ev.disk_info.get("install_command"),
            "root_filesystem": ev.disk_info.get("root_fs"),
            "root_filesystem_source": ev.disk_info.get("root_fs_source"),
            "guest_block_devices": devices,
            "guest_mounts": {
                "/": root,
                "/sysroot": sysroot,
                "/var": mount_of(ev, "findmnt-var.json"),
                "/boot": mount_of(ev, "findmnt-boot.json"),
            },
        },
        "bootc": {
            "version": ev.line("bootc-version.txt") or None,
            "booted_image": booted_image,
            "booted_image_digest": (booted.get("image") or {}).get("imageDigest"),
            "ostree_checksum": (booted.get("ostree") or {}).get("checksum"),
            "stateroot": (booted.get("ostree") or {}).get("stateroot"),
            "rollback_deployment": (bootc.get("status") or {}).get("rollback"),
        },
        "display": {
            "display_manager": dm.get("Id"),
            "greeter_pids": {"first": first or None, "after_hold": second or None},
            "drm": ev.text("drm.txt").strip() or None,
            "software_rendering_evidence": [
                " ".join(line.split()) for line in ev.text("renderer-journal.txt").splitlines() if line.strip()
            ][:10],
            "screenshot": shot or None,
        },
        "network": {
            "addresses": addresses,
            "gateway": st.get("gateway"),
            "gateway_ping": st.get("gateway_ping"),
            "dns_lookup": st.get("dns_lookup"),
            "networkmanager": ev.line("nmcli-general.txt") or None,
        },
        "selinux": {"mode": enforce or None, "avc_denials": len(avc)},
        "vm": {
            "outcome": vm.get("outcome"),
            "qemu": vm.get("qemu_version"),
            "kvm": vm.get("kvm"),
            "firmware": vm.get("firmware"),
            "guest": vm.get("guest"),
            "shutdown": vm.get("shutdown"),
        },
        "runner_kvm": ev.kvm or None,
        "guest_report_source": ev.guest_source,
        "timings_seconds": timings,
        "checks": checks.items,
    }
    (out / "boot-report.json").write_text(json.dumps(report, indent=2) + "\n")

    # ---- summary ----------------------------------------------------------------------
    icon = {"pass": "pass", "fail": "**FAIL**", "not-run": "**NOT RUN**"}
    lines = [
        f"### UOS boot test: {report['result'].upper()}",
        "",
        "| Check | Result | Evidence |",
        "|---|---|---|",
    ]
    for item in checks.items:
        name = item["description"] + ("" if item["required"] else " (informational)")
        lines.append(f"| {name} | {icon[item['result']]} | {item['detail'].replace('|', '/')} |")
    facts = [
        ("Build ID", report["build_id"]),
        ("Kernel", report["kernel"]),
        ("Bootloader", loader["in_use"]),
        ("Firmware boot entry", loader["efi_boot_entry"]),
        ("Secure Boot", loader["secure_boot"]),
        ("Boot to graphical.target", f"{time_to_graphical} s (kernel start, guest clock)"),
        ("systemd-analyze", report["boot"]["systemd_analyze"]),
        ("System state", st.get("system_state")),
        ("Failed units", ", ".join(failed) or "none"),
        ("SELinux", f"{enforce or 'no data'}, {len(avc)} AVC denials in the journal"),
        ("Root filesystem", f"{ev.disk_info.get('root_fs')} ({ev.disk_info.get('root_fs_source')})"),
        ("bootc", f"{report['bootc']['version']}; booted {booted_image}"),
        ("QEMU", f"{vm.get('qemu_version')}; KVM {vm.get('kvm')}"),
        ("Guest report came by", ev.guest_source),
        ("Timings (s)", ", ".join(f"{k}={v}" for k, v in timings.items())),
    ]
    lines += ["", "| | |", "|---|---|"]
    lines += [f"| {key} | `{str(value).replace('|', '/')}` |" for key, value in facts]
    lines += [
        "",
        "The guest renders in software on a virtual display. This shows that the session starts;",
        "it says nothing about GPU drivers, performance or real hardware.",
        "",
    ]
    summary = "\n".join(lines)
    (out / "summary.md").write_text(summary)
    print(summary)
    return 0 if checks.passed() else 1


if __name__ == "__main__":
    sys.exit(main())
