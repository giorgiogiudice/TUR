#!/usr/bin/env python3
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
"""Turn the evidence of a boot test into boot-report.json and a summary.

    tests/boot/report.py --work WORK_DIR --out OUT_DIR --expected-build-id ID
                         [--expected-failed-units FILE] [--timing NAME=SECONDS ...]

Evidence, all optional (a missing piece fails the checks that need it):

    WORK_DIR/report.tar               facts sent by the guest agent, one
                                      archive per boot (first-boot, second-boot)
    OUT_DIR/guest-from-disk/          the same facts recovered from the disk
    OUT_DIR/serial.log                serial console, both boots
    OUT_DIR/screenshot-<boot>.png     virtual display
    OUT_DIR/host/vm-result.json       what tests/boot/run-vm.py saw
    OUT_DIR/host/disk-layout.json     what tests/boot/inspect-disk.py measured
    OUT_DIR/host/disk-info.env        what tests/boot/build-disk.sh did
    OUT_DIR/host/kvm.json             tests/boot/check-kvm.py

Writes OUT_DIR/boot-report.json, OUT_DIR/summary.md and, per boot,
OUT_DIR/journal-<boot>.txt and OUT_DIR/journal-<boot>.export.gz. Prints the
summary and exits 0 only if every required check passed. A check is never
downgraded to make a run pass: one that could not be evaluated is reported as
"not-run" and counts as a failure when required.
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
BOOTS = ["first-boot", "second-boot"]
SOFTWARE_RENDERERS = ("llvmpipe", "softpipe", "swrast", "lavapipe")


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


def load_json(path: pathlib.Path):
    try:
        return json.loads(path.read_bytes().decode("utf-8", errors="replace"))
    except (OSError, ValueError):
        return None


class Guest:
    """The facts the agent recorded during one boot."""

    def __init__(self, directory: pathlib.Path):
        self.dir = directory
        self.status = props(self.text("status"))

    def __bool__(self) -> bool:
        return bool(self.status)

    def text(self, name: str) -> str:
        try:
            return (self.dir / name).read_bytes().decode("utf-8", errors="replace")
        except OSError:
            return ""

    def line(self, name: str) -> str:
        text = self.text(name).strip()
        return text.splitlines()[0].strip() if text else ""

    def json(self, name: str):
        return load_json(self.dir / name)


class Evidence:
    def __init__(self, work: pathlib.Path, out: pathlib.Path):
        self.out = out
        guest_dir = out / "guest"
        self.guest_source = "none"
        tar_path = work / "report.tar"
        if tar_path.exists() and tar_path.stat().st_size > 0:
            try:
                guest_dir.mkdir(parents=True, exist_ok=True)
                # One archive per boot, written back to back on the same channel.
                data = io.BytesIO(tar_path.read_bytes())
                with tarfile.open(fileobj=data, mode="r:", ignore_zeros=True) as archive:
                    archive.extractall(guest_dir, filter="data")
                self.guest_source = "virtio-serial"
            except (tarfile.TarError, OSError, EOFError) as error:
                print(f"report.tar could not be read completely: {error}")
                self.guest_source = "virtio-serial (incomplete)"
        for boot in BOOTS:
            recovered = out / "guest-from-disk" / boot
            if not (guest_dir / boot / "status").exists() and (recovered / "status").exists():
                shutil.copytree(recovered, guest_dir / boot, dirs_exist_ok=True)
                self.guest_source += f"; {boot} from disk (post-mortem)"
        self.boots = {boot: Guest(guest_dir / boot) for boot in BOOTS}
        self.vm = load_json(out / "host" / "vm-result.json") or {}
        self.layout = load_json(out / "host" / "disk-layout.json") or {}
        self.kvm = load_json(out / "host" / "kvm.json") or {}
        info = out / "host" / "disk-info.env"
        self.disk_info = props(info.read_text()) if info.exists() else {}
        serial = out / "serial.log"
        self.serial = serial.read_bytes().decode("utf-8", errors="replace") if serial.exists() else ""


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


def firmware_entry(guest: Guest) -> dict:
    """The firmware boot entry this boot came through, from efibootmgr."""
    efibootmgr = guest.text("efibootmgr.txt")
    current = re.search(r"^BootCurrent:\s*([0-9A-Fa-f]{4})", efibootmgr, re.M)
    order = re.search(r"^BootOrder:\s*(\S+)", efibootmgr, re.M)
    entry = None
    if current:
        match = re.search(rf"^Boot{current.group(1)}\*?\s+(.*)$", efibootmgr, re.M)
        entry = " ".join(match.group(1).split()) if match else None
    with_files = []
    for number, rest in re.findall(r"^Boot([0-9A-Fa-f]{4})\*?\s+(.*\\EFI\\.*)$", efibootmgr, re.M):
        with_files.append(f"Boot{number} {' '.join(rest.split())}")
    binary = re.search(r"\\EFI\\[^\s)]+", entry) if entry else None
    return {
        "boot_current": current.group(1) if current else None,
        "boot_order": order.group(1) if order else None,
        "entry": entry,
        "efi_binary": binary.group(0) if binary else None,
        "entries_naming_a_file": with_files,
    }


def boot_loader(ev: Evidence) -> dict:
    """What booted the system, from firmware variables, the ESP and bootupd."""
    first, second = ev.boots["first-boot"], ev.boots["second-boot"]
    guest = first or second
    bootctl = guest.text("bootctl-status.txt")
    product = re.search(r"Current Boot Loader:\s*\n\s*Product:\s*(.+)", bootctl)
    features = re.findall(r"([✓✗])\s+(.+)$", bootctl, re.M)
    secure = re.search(r"^\s*Secure Boot:\s*(.+)$", bootctl, re.M)
    esp_files = [name.lower() for name in ev.layout.get("esp_files", [])]
    has_shim = any(name.endswith("/shimx64.efi") for name in esp_files)
    has_grub = any(name.endswith("/grubx64.efi") for name in esp_files)
    has_sdboot = any("systemd-boot" in name for name in esp_files)
    loader_info = guest.status.get("loader_info", "") or (product.group(1).strip() if product else "")
    if "grub" in loader_info.lower() or (has_grub and not has_sdboot):
        name = "GRUB 2, loaded by shim" if has_shim else "GRUB 2"
    elif "systemd-boot" in loader_info.lower() or has_sdboot:
        name = "systemd-boot"
    else:
        name = None
    return {
        "in_use": name,
        "loader_info": loader_info or None,
        "loader_features": {text.strip(): mark == "✓" for mark, text in features} or None,
        "secure_boot": (secure.group(1).strip() if secure else None) or guest.line("secure-boot.txt") or None,
        "firmware_entry_first_boot": firmware_entry(first),
        "firmware_entry_second_boot": firmware_entry(second),
        "esp_has_shim": has_shim,
        "esp_has_grub": has_grub,
        "esp_has_systemd_boot": has_sdboot,
        "grub_seen_on_serial_console": "GRUB" in ev.serial[:400_000],
        "boot_loader_entries": ev.layout.get("boot_entries"),
        "grub_config_mentions_boot_counter": ev.layout.get("grub_config_mentions_boot_counter"),
        "grub_environment": guest.text("grubenv.txt").split() or None,
        "bootupd_status": guest.text("bootupctl-status.txt").strip() or None,
        "kernel_command_line": guest.line("cmdline.txt") or None,
    }


def mount_of(guest: Guest, name: str) -> dict | None:
    data = guest.json(name)
    try:
        fs = data["filesystems"][0]
        return {"source": fs.get("source"), "fstype": fs.get("fstype"), "options": fs.get("options")}
    except (TypeError, KeyError, IndexError):
        return None


def guest_block_devices(guest: Guest) -> list[dict]:
    data = guest.json("lsblk.json") or {}
    devices = []
    for device in data.get("blockdevices", []):
        if device.get("type") != "disk" or not device.get("children"):
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


def renderer_of(guest: Guest) -> dict:
    """What KWin says it renders with, from its support information."""
    info = guest.text("kwin-support-information.txt")
    wanted = re.compile(r"renderer string|OpenGL vendor string|OpenGL version string|Compositing Type|GPU class|^Driver:", re.I)
    lines = [" ".join(line.split()) for line in info.splitlines() if wanted.search(line)]
    renderer = next((line.split(":", 1)[1].strip() for line in lines if "renderer string" in line.lower()), None)
    software = None
    if renderer:
        software = any(name in renderer.lower() for name in SOFTWARE_RENDERERS)
    return {"renderer": renderer, "software": software, "kwin_reports": lines[:12]}


def check_boot(ev: Evidence, boot: str, checks: Checks, args, allowed: dict[str, str]) -> dict:
    """Evaluate one boot; return its section of the report."""
    guest = ev.boots[boot]
    st, vm, out = guest.status, ev.vm, ev.out
    label = boot.replace("-", " ").capitalize()

    def add(ident: str, description: str, passed, detail: str, required: bool = True) -> None:
        checks.add(f"{boot}.{ident}", f"{label}: {description}", passed, detail, required)

    for name in ("journal.txt", "journal.export.gz"):
        if (guest.dir / name).exists():
            shutil.move(guest.dir / name, out / name.replace("journal", f"journal-{boot}", 1))

    build_id = st.get("build_id", "")
    add(
        "image_identity",
        "the booted system is the image built from this commit",
        (build_id == args.expected_build_id and bool(build_id)) if guest else None,
        f"booted BUILD_ID={build_id or 'no data'}; expected {args.expected_build_id or 'not given'}",
    )
    kernel = st.get("kernel", "")
    add(
        "kernel_and_firmware",
        "UEFI boot, running the kernel shipped in the image",
        (bool(kernel) and st.get("kernel_modules_match") == "yes" and st.get("uefi") == "yes") if guest else None,
        f"uname -r={kernel or 'no data'}; modules for it present: {st.get('kernel_modules_match', 'no data')}; "
        f"/sys/firmware/efi present: {st.get('uefi', 'no data')}",
    )

    # ---- graphical target, display manager, session ----------------------------------
    graphical = props(guest.text("unit-graphical.txt"))
    graphical_us = int(graphical.get("ActiveEnterTimestampMonotonic", "0") or 0)
    time_to_graphical = round(graphical_us / 1e6, 3) if graphical_us else None
    add(
        "graphical_target",
        "systemd reached graphical.target",
        (st.get("graphical_target") == "reached" and graphical.get("ActiveState") == "active") if guest else None,
        f"agent wait: {st.get('graphical_target', 'no data')}; ActiveState={graphical.get('ActiveState', 'no data')}; "
        f"{time_to_graphical} s after kernel start",
    )
    add(
        "boot_time",
        "the time from kernel start to graphical.target was measured",
        (time_to_graphical is not None) if guest else None,
        f"{time_to_graphical} s" if time_to_graphical is not None else "no data",
    )
    dm = props(guest.text("unit-dm.txt"))
    alias = props(guest.text("unit-dm-alias.txt"))
    failure = props(guest.text("unit-dm-failure.txt"))
    failure_ran = failure.get("ExecMainStartTimestampMonotonic", "0") not in ("0", "")
    dm_ok = (
        dm.get("ActiveState") == "active"
        and dm.get("SubState") == "running"
        and dm.get("NRestarts") == "0"
        and alias.get("Id") == dm.get("Id")
        and st.get("display_manager_after_hold") == "active"
        and not failure_ran
    )
    add(
        "display_manager",
        "the display manager started and stayed up",
        dm_ok if dm else None,
        f"{dm.get('Id', 'no data')}: {dm.get('ActiveState', '?')}/{dm.get('SubState', '?')}, "
        f"restarts={dm.get('NRestarts', '?')}; display-manager.service is {alias.get('Id', '?')}; "
        f"failure handler ran: {failure_ran}",
    )
    session = {
        "id": st.get("session_id") or None,
        "user": st.get("session_user") or None,
        "class": st.get("session_class") or None,
        "type": st.get("session_type") or None,
        "state": st.get("session_state") or None,
        "service": st.get("session_service") or None,
        "seat": st.get("session_seat") or None,
        "compositor_pid": st.get("compositor_pid") or None,
        "compositor_pid_after_hold": st.get("compositor_pid_after_hold") or None,
    }
    session_ok = (
        st.get("graphical_session") == "up"
        and session["type"] in ("wayland", "x11")
        and session["state"] == "active"
        and session["seat"] == "seat0"
        and (session["service"] or "").startswith("plasmalogin")
        and bool(session["compositor_pid"])
        and session["compositor_pid"] == session["compositor_pid_after_hold"]
        and st.get("session_id") == st.get("session_id_after_hold")
    )
    add(
        "graphical_session",
        "a graphical session started by the display manager owns the seat, with its compositor still running after a hold",
        session_ok if guest else None,
        f"agent wait: {st.get('graphical_session', 'no data')}; session {session['id']} user={session['user']} "
        f"class={session['class']} type={session['type']} state={session['state']} service={session['service']}; "
        f"kwin_wayland PID {session['compositor_pid']} then {session['compositor_pid_after_hold']}",
    )
    if boot == "second-boot":
        add(
            "login_greeter",
            "with first-run setup marked complete, the session is the display manager's login greeter",
            (session["class"] == "greeter") if guest else None,
            f"session class={session['class']} user={session['user']} service={session['service']}",
        )
    renderer = renderer_of(guest)
    add(
        "software_rendering",
        "the compositor reports a software renderer, as expected without a GPU",
        renderer["software"],
        f"KWin: {renderer['renderer'] or 'no renderer reported'}",
        required=False,
    )
    shot = (vm.get("screenshots") or {}).get(boot) or {}
    add(
        "screenshot",
        "a screenshot of the virtual display shows rendered content, not a black or text screen",
        (
            bool(shot.get("looks_rendered"))
            and (out / f"screenshot-{boot}.png").exists()
            and shot.get("taken") == "after the agent reported"
        )
        if shot
        else None,
        f"{shot.get('width', '?')}x{shot.get('height', '?')}, lit fraction {shot.get('lit_fraction', '?')}, "
        f"{shot.get('distinct_colours', '?')} colours sampled, taken {shot.get('taken', 'never')}",
    )

    # ---- network -----------------------------------------------------------------------
    addresses = []
    for link in guest.json("ip-addr.json") or []:
        if link.get("ifname") == "lo":
            continue
        for addr in link.get("addr_info", []):
            if addr.get("family") == "inet" and addr.get("scope") == "global":
                addresses.append(f"{link.get('ifname')} {addr.get('local')}/{addr.get('prefixlen')}")
    add(
        "network",
        "the guest has an IPv4 address, a default route and a reachable gateway",
        (bool(addresses) and st.get("default_route") == "yes" and st.get("gateway_ping") == "ok") if guest else None,
        f"addresses: {', '.join(addresses) or 'none'}; default route: {st.get('default_route', 'no data')}; "
        f"gateway {st.get('gateway', '?')} ping: {st.get('gateway_ping', 'no data')}; "
        f"NetworkManager: {guest.line('nmcli-general.txt') or 'no data'}",
    )

    # ---- storage and bootc ---------------------------------------------------------------
    devices = guest_block_devices(guest)
    sysroot = mount_of(guest, "findmnt-sysroot.json")
    parts = [part for device in devices for part in device["partitions"]]
    has_esp = any((part.get("parttype") or "").lower() == ESP_GUID for part in parts)
    sysroot_part = next((part for part in parts if "/sysroot" in part["mountpoints"]), None)
    add(
        "storage",
        "the disk is partitioned as installed and the physical root is mounted at /sysroot",
        (has_esp and sysroot is not None and sysroot_part is not None) if guest else None,
        f"/sysroot: {sysroot}; EFI system partition seen: {has_esp}; "
        f"partitions: {', '.join(str(p.get('partlabel') or p.get('name')) for p in parts) or 'no data'}",
    )
    bootc = guest.json("bootc-status.json") or {}
    booted = ((bootc.get("status") or {}).get("booted")) or {}
    booted_image = ((booted.get("image") or {}).get("image") or {}).get("image")
    test_image = ev.disk_info.get("test_image", "")
    add(
        "bootc_status",
        "bootc reports a booted deployment of the installed image",
        (bool(booted_image) and (not test_image or booted_image == test_image)) if guest else None,
        f"booted image: {booted_image or 'no data'}; installed from: {test_image or 'unknown'}; "
        f"host type: {(bootc.get('status') or {}).get('type', '?')}",
    )

    # ---- health --------------------------------------------------------------------------
    failed_list = guest.json("failed-units.json")
    failed = [unit.get("unit") for unit in (failed_list or []) if unit.get("unit")]
    unexpected = [unit for unit in failed if unit not in allowed]
    add(
        "failed_units",
        "no systemd unit failed, other than those listed in tests/boot/expected-failed-units.txt",
        (not unexpected) if isinstance(failed_list, list) else None,
        f"failed: {', '.join(failed) or 'none'}; not listed: {', '.join(unexpected) or 'none'}",
    )
    enforce = guest.line("getenforce.txt")
    avc = [line for line in guest.text("avc-denials.txt").splitlines() if "avc:" in line]
    add(
        "selinux",
        "SELinux is enforcing",
        (enforce == "Enforcing") if guest else None,
        f"getenforce: {enforce or 'no data'}; {len(avc)} AVC denials in the journal",
    )
    journal = out / f"journal-{boot}.txt"
    export = out / f"journal-{boot}.export.gz"
    journal_lines = sum(1 for _ in open(journal, errors="replace")) if journal.exists() else 0
    add(
        "journal",
        "the journal of this boot was exported",
        journal_lines > 200 and export.exists(),
        f"{journal.name}: {journal_lines} lines; export: {export.stat().st_size if export.exists() else 0} bytes",
    )

    analyze = guest.text("systemd-analyze-time.txt")
    phases = {}
    for value, phase in re.findall(
        r"((?:[0-9.]+(?:min|ms|us|s|h)\s*)+)\((firmware|loader|kernel|initrd|userspace)\)", analyze
    ):
        phases[phase] = span_seconds(value)
    dm_us = int(dm.get("ActiveEnterTimestampMonotonic", "0") or 0)
    chain = [line.rstrip() for line in guest.text("systemd-analyze-critical-chain.txt").splitlines() if "@" in line and "└" in line or line.startswith("graphical.target")]
    return {
        "name": boot,
        "observed": bool(guest),
        "boot_id": st.get("boot_id"),
        "build_id": build_id or None,
        "kernel": kernel or None,
        "time_to_graphical_target_seconds": time_to_graphical,
        "time_to_display_manager_seconds": round(dm_us / 1e6, 3) if dm_us else None,
        "systemd_analyze": " ".join(analyze.split()) or None,
        "phases_seconds": phases or None,
        "critical_chain": chain[:8],
        "system_state": st.get("system_state"),
        "failed_units": failed,
        "unexpected_failed_units": unexpected,
        "display_manager": dm.get("Id"),
        "session": session,
        "renderer": renderer,
        "screenshot": shot or None,
        "network": {
            "addresses": addresses,
            "gateway": st.get("gateway"),
            "gateway_ping": st.get("gateway_ping"),
            "dns_lookup": st.get("dns_lookup"),
            "networkmanager": guest.line("nmcli-general.txt") or None,
        },
        "selinux": {"mode": enforce or None, "avc_denials": len(avc)},
        "mounts": {
            "/": mount_of(guest, "findmnt-root.json"),
            "/sysroot": sysroot,
            "/var": mount_of(guest, "findmnt-var.json"),
            "/boot": mount_of(guest, "findmnt-boot.json"),
        },
        "block_devices": devices,
        "bootc": {
            "version": guest.line("bootc-version.txt") or None,
            "booted_image": booted_image,
            "booted_image_digest": (booted.get("image") or {}).get("imageDigest"),
            "store": booted.get("store"),
            "ostree_checksum": (booted.get("ostree") or {}).get("checksum"),
            "stateroot": (booted.get("ostree") or {}).get("stateroot"),
            "rollback_deployment": (bootc.get("status") or {}).get("rollback"),
        },
        "second_boot_prepared": st.get("second_boot_prepared"),
    }


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
    vm = ev.vm
    first, second = ev.boots["first-boot"], ev.boots["second-boot"]
    allowed = expected_failures(pathlib.Path(args.expected_failed_units) if args.expected_failed_units else None)
    checks = Checks()

    # ---- the virtual machine and what was installed -----------------------------------
    kvm_enabled = (vm.get("kvm") or {}).get("enabled")
    virt = first.line("virt.txt") or second.line("virt.txt")
    checks.add(
        "kvm",
        "The guest ran under KVM acceleration, not software emulation",
        (bool(kvm_enabled) and virt == "kvm") if vm else None,
        f"QEMU query-kvm enabled={kvm_enabled}; guest systemd-detect-virt={virt or 'no data'}",
    )
    banners = len(re.findall(r"Linux version \d", ev.serial))
    starts = len(re.findall(re.escape(MARK) + "START", ev.serial))
    checks.add(
        "serial_console",
        "The serial console was captured from firmware to the test agent, for both boots",
        bool(ev.serial) and banners >= 2 and starts >= 2,
        f"{len(ev.serial)} bytes; kernel banners: {banners}; agent start markers: {starts}",
    )
    boot_ids = {guest.status.get("boot_id") for guest in (first, second) if guest}
    checks.add(
        "two_boots",
        "The same disk booted twice: as installed, and again after a reboot",
        (len(boot_ids) == 2 and first.status.get("second_boot_prepared") == "yes") if (first or second) else None,
        f"distinct boot IDs: {len(boot_ids)}; second boot prepared by the agent: "
        f"{first.status.get('second_boot_prepared', 'no data')}; kernel banners on serial: {banners}",
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
    entry_first = loader["firmware_entry_first_boot"]["entry"]
    entry_second = loader["firmware_entry_second_boot"]["entry"]
    checks.add(
        "bootloader",
        "The bootloader that started the system is identified",
        (bool(loader["in_use"]) and bool(entry_first) and bool(loader["boot_loader_entries"]))
        if (first or ev.layout)
        else None,
        f"{loader['in_use'] or 'not identified'}; LoaderInfo: {loader['loader_info'] or 'not set'}; "
        f"first boot through: {entry_first or 'no data'}; second boot through: {entry_second or 'no data'}; "
        f"boot loader entries: {loader['boot_loader_entries']}",
    )

    boots = [check_boot(ev, boot, checks, args, allowed) for boot in BOOTS]

    checks.add(
        "clean_shutdown",
        "The guest powered off when asked",
        (vm.get("shutdown") == "clean") if vm.get("shutdown") else None,
        f"shutdown: {vm.get('shutdown', 'not attempted')}",
        required=False,
    )

    # ---- report ------------------------------------------------------------------------
    uos_release = props(first.text("uos-release") or second.text("uos-release"))
    timings = {}
    for item in args.timing:
        name, _, value = item.partition("=")
        try:
            timings[name] = float(value)
        except ValueError:
            timings[name] = value or None
    if "install_seconds" in ev.disk_info:
        timings["disk_install_seconds"] = float(ev.disk_info["install_seconds"])
    timings["vm_seconds"] = vm.get("elapsed_seconds")
    failed_all = sorted({unit for boot in boots for unit in boot["failed_units"]})
    report = {
        "schema_version": 1,
        "task": "UOS-003",
        "result": "pass" if checks.passed() else "fail",
        "build_id": boots[0]["build_id"] or boots[1]["build_id"],
        "expected_build_id": args.expected_build_id or None,
        "source_revision": uos_release.get("UOS_SOURCE_REVISION"),
        "kernel": boots[0]["kernel"] or boots[1]["kernel"],
        "boot_time_to_graphical_target_seconds": {
            boot["name"]: boot["time_to_graphical_target_seconds"] for boot in boots
        },
        "boot_time_note": "kernel start to graphical.target active, guest monotonic clock; firmware and bootloader time excluded",
        "failed_units": failed_all,
        "expected_failed_units": allowed,
        "unexpected_failed_units": sorted({unit for boot in boots for unit in boot["unexpected_failed_units"]}),
        "bootloader": loader,
        "partition_layout": {
            "installed": ev.layout or None,
            "install_command": ev.disk_info.get("install_command"),
            "root_filesystem": ev.disk_info.get("root_fs"),
            "root_filesystem_source": ev.disk_info.get("root_fs_source"),
        },
        "boots": boots,
        "vm": {
            "outcome": vm.get("outcome"),
            "qemu": vm.get("qemu_version"),
            "kvm": vm.get("kvm"),
            "firmware": vm.get("firmware"),
            "guest": vm.get("guest"),
            "shutdown": vm.get("shutdown"),
            "host_wall_clock_seconds": vm.get("milestones"),
            "boots_seen": vm.get("boots_seen"),
            "firmware_resets_seen": vm.get("resets_seen"),
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

    def per_boot(key, fmt=str):
        return "; ".join(f"{boot['name']}: {fmt(boot[key])}" for boot in boots)

    def failed_text(units):
        return ", ".join(f"{u} (listed as expected)" if u in allowed else u for u in units) or "none"

    facts = [
        ("Build ID", report["build_id"]),
        ("Kernel", report["kernel"]),
        ("Bootloader", loader["in_use"]),
        ("Secure Boot", loader["secure_boot"]),
        ("Boot to graphical.target (kernel start, guest clock)", per_boot("time_to_graphical_target_seconds", lambda v: f"{v} s")),
        ("systemd-analyze", per_boot("systemd_analyze")),
        ("System state", per_boot("system_state")),
        ("Failed units", per_boot("failed_units", failed_text)),
        ("Session on the seat", per_boot("session", lambda s: f"user {s['user']}, class {s['class']}, {s['type']}")),
        ("Renderer reported by KWin", per_boot("renderer", lambda r: r["renderer"])),
        ("SELinux", per_boot("selinux", lambda s: f"{s['mode']}, {s['avc_denials']} AVC denials")),
        ("Root filesystem", f"{ev.disk_info.get('root_fs')} ({ev.disk_info.get('root_fs_source')})"),
        ("bootc", f"{boots[0]['bootc']['version']}; booted {boots[0]['bootc']['booted_image']}"),
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
