#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Turn the evidence of a boot test into boot-report.json and a summary.

    tests/boot/report.py --work WORK_DIR --out OUT_DIR --expected-build-id ID
                         [--expected-failed-units FILE] [--timing NAME=SECONDS ...]

Evidence, all optional (a missing piece fails the checks that need it):

    WORK_DIR/report.tar               facts sent by the guest agent, one
                                      archive per boot (first-boot, second-boot,
                                      third-boot)
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

MARK = "TUR-BOOT-TEST: "
ESP_GUID = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"
BOOTS = ["first-boot", "second-boot", "third-boot"]
SOFTWARE_RENDERERS = ("llvmpipe", "softpipe", "swrast", "lavapipe")
ROOT = pathlib.Path(__file__).resolve().parents[2]


def image_programs() -> tuple[list[str], list[str]]:
    """The programs image/programs.tsv puts into the image, and the components
    among them that ship assistant tools (agent-tools.json at the top of the
    component), as the build installs them (image/programs.sh)."""
    programs, tool_sets = [], []
    for line in (ROOT / "image" / "programs.tsv").read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        component, program, _ = line.split("\t")
        programs.append(program)
        if (ROOT / component / "agent-tools.json").is_file():
            tool_sets.append(pathlib.Path(component).name)
    return programs, sorted(tool_sets)


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
        "esp_efi_binaries": ev.layout.get("esp_efi_binaries"),
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
    if boot != "first-boot":
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

    if boot == "second-boot":
        after = (vm.get("screenshots") or {}).get("second-boot-after-input") or {}
        changed = after.get("changed_fraction")
        add(
            "display_responds",
            "the display changes after a key press and pointer movement",
            (changed > 0.01) if isinstance(changed, (int, float)) else None,
            f"share of sampled pixels that changed: {changed if changed is not None else 'no data'}",
            required=False,
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
        "screenshot_after_input": ((vm.get("screenshots") or {}).get("second-boot-after-input") or None)
        if boot == "second-boot"
        else None,
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


def check_components(ev: Evidence, checks: Checks, args) -> dict:
    """TUR's own programs on the installed system: what component-checks recorded in the second boot.

    The programs ran as an ordinary user in that user's session, under
    SELinux in enforcing mode. A check here says what a program did on this
    virtual machine; none says anything about real hardware.
    """
    guest = ev.boots["second-boot"]
    base = guest.dir / "components"

    def text(name: str) -> str:
        try:
            return (base / name).read_bytes().decode("utf-8", errors="replace")
        except OSError:
            return ""

    def document(name: str):
        value = load_json(base / name)
        return value if isinstance(value, (dict, list)) else None

    status = props(text("status"))
    codes: dict[str, int] = {}
    for line in text("exit-status.tsv").splitlines():
        name, _, code = line.partition("\t")
        if code.strip().lstrip("-").isdigit():
            codes[name] = int(code)
    ran = guest.status.get("components_finished") == "yes" and bool(status)

    def add(ident: str, description: str, passed, detail: str, required: bool = True) -> None:
        checks.add(f"components.{ident}", f"TUR programs: {description}", passed if ran else None, detail, required)

    def first_line(name: str, limit: int = 300) -> str:
        lines = [line for line in text(name).splitlines() if line.strip()]
        return lines[0][:limit] if lines else ""

    # ---- how they were run ---------------------------------------------------------------
    context = status.get("selinux_context", "")
    checks.add(
        "components.session",
        "TUR programs: they ran on the installed system as an ordinary user, in a login session of that user, "
        "with SELinux enforcing",
        (
            ran
            # Under the user's own service manager, or in a session the
            # system's service manager opened through PAM. Not "runuser",
            # which gives the user's identity and nothing else.
            and guest.status.get("components_run_as") in ("user-session", "pam-login")
            and status.get("selinux_mode") == "Enforcing"
            and status.get("uid") not in ("", "0", None)
            and bool(status.get("xdg_runtime_dir"))
        )
        if guest
        else None,
        f"finished: {guest.status.get('components_finished', 'no data')}; run as: "
        f"{guest.status.get('components_run_as', 'no data')}; user {status.get('user')} (uid {status.get('uid')}); "
        f"SELinux {status.get('selinux_mode')}, context {context or 'no data'}; home {status.get('home')} -> "
        f"{status.get('home_resolved')}; runtime directory {status.get('xdg_runtime_dir')}; from "
        f"{guest.status.get('components_started_uptime')} s to {guest.status.get('components_finished_uptime')} s "
        f"after boot",
    )
    add(
        "user_service_manager",
        "they ran under the user's own service manager, as the programs of a desktop session do",
        guest.status.get("components_run_as") == "user-session",
        f"run as: {guest.status.get('components_run_as', 'no data')} (user-session: a unit of the user's service "
        "manager; pam-login: a session the system's service manager opened through PAM); "
        f"{' / '.join(line.strip() for line in guest.text('components-session.txt').splitlines() if 'Failed' in line)[:300]}",
        required=False,
    )
    add(
        "selinux_context",
        "the user's processes run in the SELinux domain of a login (unconfined_t), not in a service's",
        ":unconfined_r:unconfined_t:" in context,
        f"id -Z: {context or 'no data'}; with sudo: {first_line('context-sudo-label.out') or 'no data'}",
    )

    # ---- every program is there and runs ---------------------------------------------------
    expected_programs, expected_tool_sets = image_programs()
    versions = [line.split("\t") for line in text("versions.tsv").splitlines() if line.strip()]
    bad = [row[0] for row in versions if len(row) < 4 or row[2] != "0" or not row[3].startswith(row[0] + " ")]
    add(
        "versions",
        "every program of the image's list (image/programs.tsv) starts and prints its version",
        sorted(row[0] for row in versions) == sorted(expected_programs) and not bad,
        f"{len(versions)} programs: {', '.join(row[3] for row in versions if len(row) >= 4) or 'no data'}"
        + (f"; failed: {bad}" if bad else ""),
    )

    # ---- tur-diag ----------------------------------------------------------------------------
    diag = document("diag-status-json.out") or {}
    diag_root = document("diag-status-json-root.out") or {}
    add(
        "diag_status",
        "tur-diag status reports the build that is running",
        codes.get("diag-status") == 0
        and codes.get("diag-status-json") == 0
        and (diag.get("tur") or {}).get("build_id") == args.expected_build_id
        and diag.get("kernel_release") == guest.status.get("kernel")
        and isinstance(diag.get("failed_units"), int),
        f"exit status {codes.get('diag-status-json')}; build {(diag.get('tur') or {}).get('build_id')}, kernel "
        f"{diag.get('kernel_release')}, failed units {diag.get('failed_units')}, boot ID {diag.get('boot_id')}",
    )
    deployment = (diag_root.get("tur") or {}).get("deployment_checksum")
    booted = ((((guest.json("bootc-status.json") or {}).get("status") or {}).get("booted") or {}).get("ostree") or {})
    add(
        "diag_status_deployment",
        "as root, tur-diag status names the booted deployment as bootc does",
        codes.get("diag-status-json-root") == 0 and bool(deployment) and deployment == booted.get("checksum"),
        f"tur-diag: deployment {deployment}, image digest {(diag_root.get('tur') or {}).get('image_digest')}; bootc: "
        f"{booted.get('checksum')}; as the user: {(diag.get('tur') or {}).get('deployment_checksum')}",
    )
    bundles = {}
    for who in ("user", "root"):
        manifest = document(f"bundle-{who}/manifest.json") or {}
        listing = [line.split()[-1] for line in text(f"diag-bundle-{who}-listing.out").splitlines() if line.strip()]
        missing = [f"{entry.get('path')}: {entry.get('reason')}" for entry in manifest.get("missing") or []]
        bundles[who] = {
            "collect_exit_status": codes.get(f"diag-collect-{who}"),
            "verify_exit_status": codes.get(f"diag-verify-{who}"),
            "bytes": status.get(f"bundle_{who}_bytes"),
            "members": listing,
            "files": len(manifest.get("files") or []),
            "not_collected": missing,
            "redaction_profile": manifest.get("redaction_profile"),
            "boot_health_journal_records": status.get(f"bundle_{who}_boot_health_journal_records"),
            "journal_lines": status.get(f"bundle_{who}_journal_lines"),
        }
        add(
            f"diag_collect_{who}",
            f"tur-diag collect, run {'as root' if who == 'root' else 'as the user'}, writes a bundle that "
            "verify-bundle accepts",
            codes.get(f"diag-collect-{who}") == 0
            and codes.get(f"diag-verify-{who}") == 0
            and "manifest.json" in listing
            and "checksums.sha256" in listing,
            f"collect exit status {codes.get(f'diag-collect-{who}')}, verify-bundle "
            f"{codes.get(f'diag-verify-{who}')}; {status.get(f'bundle_{who}_bytes')} bytes, {len(listing)} members, "
            f"{bundles[who]['journal_lines']} journal lines; not collected ({len(missing)}): "
            f"{'; '.join(missing) or 'nothing'}",
        )
    add(
        "diag_verify_rejects",
        "verify-bundle refuses a bundle that was cut short",
        codes.get("diag-verify-cut") not in (None, 0),
        f"exit status {codes.get('diag-verify-cut')}: {first_line('diag-verify-cut.err') or first_line('diag-verify-cut.out')}",
    )
    health = document("bundle-root/boot/boot-health.json") or {}
    records = bundles["root"]["boot_health_journal_records"] or ""
    add(
        "diag_boot_health",
        "root's bundle holds the boot-health state: what tur-boot-health reports, and its journal records",
        "boot/boot-health.json" in bundles["root"]["members"]
        and "recent_events" in health
        and "verdict" in health
        and records.isdigit()
        and int(records) > 0,
        f"boot/boot-health.json in the bundle: {'boot/boot-health.json' in bundles['root']['members']}; its keys: "
        f"{sorted(health)[:12]}; tur-boot-health records in the bundle's journal: {records or 'no data'}; in the "
        f"user's bundle: {bundles['user']['boot_health_journal_records']}",
    )

    # ---- tur-device ----------------------------------------------------------------------------
    inventory = document("device-inventory.out") or {}
    redacted = document("device-inventory-redacted.out") or {}
    counts = {key: len(value) for key, value in inventory.items() if isinstance(value, list)}
    add(
        "device_inventory",
        "tur-device inventory describes the virtual machine's hardware",
        codes.get("device-inventory") == 0
        and inventory.get("redacted") is False
        and redacted.get("redacted") is True
        and counts.get("pci", 0) > 0
        and counts.get("block", 0) > 0
        and counts.get("network", 0) > 0,
        f"exit status {codes.get('device-inventory')}; devices by category: {counts or 'no data'}",
    )
    problems = document("device-problems.out") or {}
    problems_root = document("device-problems-root.out") or {}

    def findings(report: dict) -> list[str]:
        return [
            f"{finding.get('code')} ({finding.get('severity')}) {finding.get('device_id')}: "
            f"{str(finding.get('user_message') or finding.get('title'))[:160]}"
            for finding in report.get("findings") or []
            if isinstance(finding, dict)
        ]

    add(
        "device_problems",
        "tur-device problems reports its findings, as the user and as root",
        codes.get("device-problems") == 0
        and codes.get("device-problems-root") == 0
        and isinstance(problems.get("findings"), list)
        and isinstance(problems_root.get("findings"), list),
        f"as the user ({len(problems.get('findings') or [])}): {findings(problems) or 'none'}; as root "
        f"({len(problems_root.get('findings') or [])}): {findings(problems_root) or 'none'}",
    )
    listed = document("device-list.out") or {}
    one = document("device-get.out")
    add(
        "device_list_get",
        "tur-device list names the devices and get shows one of them",
        codes.get("device-list") == 0
        and len(listed.get("devices") or []) > 0
        and codes.get("device-get") == 0
        and isinstance(one, dict),
        f"{len(listed.get('devices') or [])} devices listed; get {status.get('first_device')}: exit status "
        f"{codes.get('device-get')}",
    )

    # ---- tur-storage ---------------------------------------------------------------------------
    volumes = document("storage-volumes.out") or {}
    found = volumes.get("volumes") or []
    filesystems = sorted({str(volume.get("filesystem")) for volume in found if isinstance(volume, dict)})
    decision = document("storage-decide.out") or {}
    add(
        "storage_volumes",
        "tur-storage volumes --live lists the volumes of this machine, and decide answers for one",
        codes.get("storage-volumes") == 0
        and volumes.get("kind") == "inventory"
        and len(found) >= 2
        and codes.get("storage-decide") == 0
        and decision.get("kind") == "decision",
        f"exit status {codes.get('storage-volumes')}; {len(found)} volumes, filesystems {filesystems}; decide "
        f"{status.get('first_volume')} read-only: exit status {codes.get('storage-decide')}, decision "
        f"{json.dumps((decision.get('volume') or {}).get('decision'))[:200]}",
    )

    # ---- tur-catalogue -------------------------------------------------------------------------
    search = document("catalogue-search.out") or {}
    route = document("catalogue-route.out") or {}
    named = document("catalogue-route-named.out") or {}
    hits = [hit.get("id") for hit in search.get("hits") or [] if isinstance(hit, dict)]
    add(
        "catalogue",
        "tur-catalogue finds the installed catalogue by itself, validates it, searches it and routes an application",
        codes.get("catalogue-validate") == 0
        and codes.get("catalogue-search") == 0
        and "com.discord.discord" in hits
        and codes.get("catalogue-route") in (0, 3, 4)
        and (route.get("application") or {}).get("id") == "com.discord.discord"
        and "recommended" in route
        and (named.get("application") or {}).get("id") == "org.libreoffice.libreoffice"
        and codes.get("catalogue-missing") == 6,
        f"validate: exit status {codes.get('catalogue-validate')} ({first_line('catalogue-validate.out')}); search "
        f"discord: {hits}; route com.discord.discord: exit status {codes.get('catalogue-route')}, recommended "
        f"{json.dumps(route.get('recommended'))[:160]}; an unknown application: exit status "
        f"{codes.get('catalogue-missing')}",
    )

    # ---- tur-runtime ---------------------------------------------------------------------------
    windows = document("runtime-inspect-windows.out") or {}
    recommendation = windows.get("recommendation") or {}
    native = (document("runtime-inspect-linux.out") or {}).get("recommendation") or {}
    absent = [line for line in text("runtime-which.out").splitlines() if line.strip()]
    add(
        "runtime_windows",
        "tur-runtime recognises a Windows program and says honestly that no runtime for it is installed",
        codes.get("runtime-inspect-windows") == 4
        and recommendation.get("runtime") == "wine"
        and "wine" in (recommendation.get("missing") or [])
        and "runtime-not-installed" in (recommendation.get("reasons") or [])
        and "is not installed on this system" in str(recommendation.get("explanation"))
        and len(absent) == 4
        and all(line.endswith(": absent") for line in absent),
        f"exit status {codes.get('runtime-inspect-windows')} (4: something is missing); runtime "
        f"{recommendation.get('runtime')}, missing {recommendation.get('missing')}, code "
        f"{recommendation.get('code')}: {str(recommendation.get('explanation'))[:200]}; in the image: "
        f"{', '.join(absent) or 'no data'}",
    )
    add(
        "runtime_linux",
        "tur-runtime recognises a Linux program as one that runs directly",
        codes.get("runtime-inspect-linux") == 0 and native.get("runtime") == "native-linux",
        f"/usr/bin/true: exit status {codes.get('runtime-inspect-linux')}, runtime {native.get('runtime')}",
    )

    # ---- tur-update ----------------------------------------------------------------------------
    update_root = document("update-status-root-json.out") or {}
    update_user = text("update-status-user.out") + text("update-status-user.err")
    system = update_root.get("system") or {}
    add(
        "update_status_root",
        "as root, tur-update status --live reads the deployments from bootc and names the running one",
        codes.get("update-status-root") == 0
        and codes.get("update-status-root-json") == 0
        and isinstance(system.get("booted"), dict)
        and bool(booted.get("checksum"))
        and str(booted.get("checksum")) in text("update-status-root-json.out"),
        f"exit status {codes.get('update-status-root-json')}; state {update_root.get('state')}: "
        f"{str(update_root.get('message'))[:160]}; versions: {len(update_root.get('versions') or [])}; boot health "
        f"given: {update_root.get('boot_health') is not None}; text: {first_line('update-status-root.out')}",
    )
    add(
        "update_status_user",
        "as an ordinary user, tur-update status --live either answers or says with its error code why it cannot",
        codes.get("update-status-user") == 0
        or (codes.get("update-status-user") == 1 and "TUR-UPDATE-" in update_user),
        f"exit status {codes.get('update-status-user')}: "
        f"{' / '.join(line.strip() for line in update_user.splitlines() if line.strip())[:400] or 'no output'}",
    )

    # ---- the installer's core, the provider layer, the tool sets -------------------------------
    disks = document("installer-inventory.out")
    add(
        "installer_inventory",
        "tur-installer inventory reads this machine's block devices (a dry run: nothing is written)",
        codes.get("installer-inventory") == 0 and disks is not None,
        f"exit status {codes.get('installer-inventory')}: "
        f"{first_line('installer-inventory.err') or (json.dumps(disks)[:200] if disks is not None else 'no output')}",
    )
    add(
        "providers_check",
        "tur-pax-providers check accepts a provider configuration (no network call is made)",
        codes.get("providers-check") == 0,
        f"exit status {codes.get('providers-check')}: "
        f"{first_line('providers-check.out') or first_line('providers-check.err')}",
    )
    manifests = {name[len("pax-manifest-"):]: code for name, code in codes.items() if name.startswith("pax-manifest-")}
    add(
        "tool_manifests",
        "tur-pax loads the assistant tools of each component of the image that ships them, as installed",
        sorted(manifests) == expected_tool_sets and all(code == 0 for code in manifests.values()),
        f"expected {', '.join(expected_tool_sets)}; "
        + ("; ".join(f"{name}: {first_line(f'pax-manifest-{name}.out') or 'exit status ' + str(code)}" for name, code in sorted(manifests.items()))
           or "no data"),
    )

    # ---- tur-apps --------------------------------------------------------------------------------
    installed = document("apps-list.out") or {}
    sources = document("apps-sources.out") or {}
    plan = document("apps-plan.out") or {}
    add(
        "apps",
        "tur-apps lists what is installed for the user and the sources, and plans an installation without "
        "doing anything",
        codes.get("apps-list") == 0
        and installed.get("kind") == "installed"
        and codes.get("apps-sources") == 0
        and sources.get("kind") == "sources"
        and codes.get("apps-plan") in (0, 3, 5, 6)
        and (plan.get("kind") == "plan" or bool((plan.get("error") or {}).get("code"))),
        f"list: exit status {codes.get('apps-list')}, {len(installed.get('applications') or [])} applications; "
        f"sources: exit status {codes.get('apps-sources')}, "
        f"{[source.get('name') for source in sources.get('sources') or [] if isinstance(source, dict)]}; plan "
        f"com.discord.discord offline: exit status {codes.get('apps-plan')}, "
        f"{plan.get('kind') or (plan.get('error') or {}).get('code')} "
        f"{json.dumps(plan.get('route') or (plan.get('error') or {}).get('message'))[:200]}; "
        f"text: {first_line('apps-plan-text.out') or first_line('apps-plan-text.err')}",
    )

    # ---- tur-ai-local and its engine ---------------------------------------------------------------
    hardware = document("ai-hardware.out") or {}
    engine = hardware.get("engine") or {}
    cpu_backends = [name for name in engine.get("backends") or [] if str(name).startswith("cpu-")]
    recommended = document("ai-recommend.out") or {}
    catalogue = document("ai-models.out") or {}
    engine_said = (text("ai-engine-version.out") + text("ai-engine-version.err")).strip()
    add(
        "ai_local",
        "tur-ai-local finds the engine the image carries, with its processor variants and Vulkan, reads the "
        "installed model catalogue and recommends a model",
        codes.get("ai-hardware") == 0
        and engine.get("present") is True
        and engine.get("vulkan_backend") is True
        and len(cpu_backends) >= 10
        and codes.get("ai-models") == 0
        and len(catalogue.get("models") or []) >= 5
        and codes.get("ai-recommend") == 0
        and codes.get("ai-engine-version") == 0,
        f"engine {engine.get('program')}: present {engine.get('present')}, backends {engine.get('backends')}; "
        f"{' / '.join(line for line in engine_said.splitlines() if line.startswith(('version', 'built with')))}; "
        f"devices: {' / '.join(line.strip() for line in text('ai-engine-devices.out').splitlines() if line.strip())[:200]}; "
        f"{len(catalogue.get('models') or [])} models in the catalogue; recommend: exit status "
        f"{codes.get('ai-recommend')}, {json.dumps(recommended.get('conversation') or recommended.get('recommended'))[:200]}; "
        f"memory {status.get('mem_total_kib')} KiB, available {status.get('mem_available_kib')} KiB",
    )

    # ---- P@x's session service --------------------------------------------------------------------
    service = document("pax-service/service-checks.json") or {}
    service_failed = [
        f"{item.get('scenario')}: {item.get('check')}: {str(item.get('detail'))[:200]}"
        for item in service.get("results") or []
        if not item.get("passed")
    ]
    add(
        "pax_service_e2e",
        "P@x's session service test (tests/pax/service_e2e.py: private bus, gnome-keyring, scripted provider) "
        "passes on the installed system with the installed tur-pax",
        codes.get("pax-service-e2e") == 0 and service.get("failed") == 0 and (service.get("passed") or 0) >= 60,
        f"exit status {codes.get('pax-service-e2e')}; {service.get('passed')} checks passed, {service.get('failed')} "
        f"failed; wake timer: {service.get('timer')}; "
        + (f"failed: {' | '.join(service_failed[:8])}" if service_failed else f"last line: {(text('pax-service-e2e.out').strip().splitlines() or ['no output'])[-1][:200]}"),
    )
    listed = document("pax-bus-models.out") or {}
    providers = [entry.get("id") for entry in listed.get("providers") or [] if isinstance(entry, dict)]
    local_offers = document("pax-bus-local.out")
    unit = props(text("pax-bus-unit.out"))
    add(
        "pax_service_on_the_session_bus",
        "the session bus starts P@x's service from the installed activation file and user unit, and it lists "
        "the installed model list's providers, the local preset among them, and local AI's models",
        codes.get("pax-bus-models") == 0
        and "local" in providers
        and codes.get("pax-bus-local") == 0
        and bool(local_offers)
        and unit.get("FragmentPath") == "/usr/lib/systemd/user/tur-pax.service",
        f"models list: exit status {codes.get('pax-bus-models')}, providers {providers}; local: exit status "
        f"{codes.get('pax-bus-local')}, {json.dumps(local_offers)[:240]}; unit: {unit}; "
        f"{first_line('pax-bus-models.err')} {first_line('pax-bus-local.err')}",
    )
    catchup: dict[str, dict[str, str]] = {}
    for block in text("pax-catchup.out").split("\n\n"):
        values = props(block)
        if values.get("Id"):
            catchup[values["Id"]] = values
    timer = catchup.get("tur-pax-catchup.timer", {})
    oneshot = catchup.get("tur-pax-catchup.service", {})
    add(
        "pax_catchup",
        "P@x's catch-up timer is enabled for the user, fired after the user's service manager started, and its "
        "command ended without an error while P@x is off",
        timer.get("ActiveState") == "active"
        and oneshot.get("Result") == "success"
        and oneshot.get("ExecMainStatus") == "0"
        and oneshot.get("ExecMainStartTimestampMonotonic", "0") not in ("", "0"),
        f"timer: {timer or 'no data'}; service: {oneshot or 'no data'}",
    )

    # ---- P@x through local AI, with real models on the engine of the image ----------------------
    local_runs = {}
    for model in status.get("local_models", "").split():
        local = document(f"pax-local-{model}/pax-local-{model}.json") or {}
        local_checks = local.get("checks") or []
        local_failed = [
            f"{item.get('name')}: {str(item.get('detail'))[:200]}"
            for item in local_checks
            if item.get("required") and not item.get("passed")
        ]
        tasks = (local.get("measurements") or {}).get("tasks") or []
        measured = [
            f"{task.get('task')}: {task.get('outcome')} in {task.get('seconds')} s, {task.get('requests')} requests, "
            f"{task.get('input_tokens')}/{task.get('output_tokens')} tokens, calls "
            f"{[(call.get('tool'), call.get('decision'), call.get('result')) for call in task.get('calls') or []]}, "
            f"answer {str(task.get('answer'))[:160]!r}"
            for task in tasks
        ]
        serve = status.get(f"local_{model}_serve")
        local_runs[model] = {
            "serve": serve,
            "exit_status": codes.get(f"pax-local-{model}"),
            "pull_seconds": status.get(f"local_{model}_pull_seconds"),
            "serve_seconds": status.get(f"local_{model}_serve_seconds"),
            "memory_available_kib": status.get(f"local_{model}_mem_available_kib"),
            "failed_checks": local_failed,
            "tasks": tasks,
        }
        add(
            f"pax_local_model_{model}",
            f"P@x asks {model} on the image's engine through the local preset (downloaded from its publisher "
            "and checked), with every installed tool set, and TUR's checks of tests/pax/local_model.py pass",
            codes.get(f"local-{model}-pull") == 0
            and serve == "ready"
            and codes.get(f"pax-local-{model}") == 0
            and bool(local_checks)
            and not local_failed,
            f"memory available before: {status.get(f'local_{model}_mem_available_kib')} KiB; pull: exit status "
            f"{codes.get(f'local-{model}-pull')} in {status.get(f'local_{model}_pull_seconds')} s; serve: {serve} "
            f"after {status.get(f'local_{model}_serve_seconds')} s ({first_line(f'local-{model}-serve.err')}); "
            f"P@x: exit status {codes.get(f'pax-local-{model}')}; "
            + (f"failed: {' | '.join(local_failed[:6])}; " if local_failed else "")
            + ("; ".join(measured) or "no task ran"),
        )
    add(
        "pax_local_models_ran",
        "P@x was run against local models on the installed system",
        bool(local_runs),
        f"models: {', '.join(local_runs) or 'none'}",
    )

    # ---- P@x end to end -------------------------------------------------------------------------
    results = document("pax/results.json") or {}
    pax_checks = results.get("checks") or []
    scenarios: dict[str, list] = {}
    for item in pax_checks:
        scenarios.setdefault(item.get("scenario"), []).append(item)
    failed = [f"{item.get('scenario')}: {item.get('check')}: {str(item.get('detail'))[:200]}" for item in pax_checks if not item.get("passed")]
    scripted = sorted(name for name in scenarios if name not in ("tool sets", "controls", "everywhere"))
    add(
        "pax_e2e",
        "the P@x end-to-end test (tests/pax/e2e.py: scripted provider, real tool programs, real sandbox) passes "
        "on the installed system",
        codes.get("pax-e2e") == 0
        and results.get("failed") == 0
        and results.get("passed", 0) >= 150
        and len(scripted) >= 17
        and results.get("real_model_called") is False,
        f"exit status {codes.get('pax-e2e')}; {results.get('passed')} checks passed, {results.get('failed')} failed, "
        f"in {len(scripted)} scenarios; no real model was called; "
        + (f"failed: {' | '.join(failed[:8])}" if failed else f"last line: {first_line('pax-e2e.err') or (text('pax-e2e.out').strip().splitlines() or ['no output'])[-1][:200]}"),
    )
    controls = {item.get("check"): item.get("passed") for item in scenarios.get("controls", [])}
    attacks = {item.get("check"): item.get("passed") for item in scenarios.get("sandbox-attacks", [])}
    probe = {item.get("check"): item.get("passed") for item in scenarios.get("sandbox-probe", [])}
    hidden = [
        "no target was reached: network, abstract socket, session bus, display, system bus",
        "no listener saw a connection from the sandbox",
        "the probe ran inside the sandbox and reported on every target",
    ]
    add(
        "pax_sandbox_sockets",
        "in the read-only sandbox no socket is reached (network, abstract socket, session bus, display, system "
        "bus), and outside it every one is: the canaries work",
        all(attacks.get(name) is True for name in hidden)
        and controls.get("outside the sandbox every canary listener is reached") is True
        and controls.get("each listener counted the control connection") is True
        and controls.get("the system bus socket exists on this machine") is True,
        f"sandbox: {[(name[:40], attacks.get(name)) for name in hidden]}; controls: "
        f"{[(name[:40], passed) for name, passed in controls.items() if 'listener' in name or 'bus' in name]}",
    )
    add(
        "pax_sandbox_confinement",
        "in the read-only sandbox nothing is written or deleted, no protected file is read, sudo gives nothing, "
        "and the environment, privileges and limits are the fixed ones",
        bool(attacks) and bool(probe) and all(attacks.values()) and all(probe.values()),
        f"sandbox-attacks: {sum(1 for value in attacks.values() if value)}/{len(attacks)} checks; sandbox-probe: "
        f"{sum(1 for value in probe.values() if value)}/{len(probe)} checks; bubblewrap and prlimit: "
        f"{' / '.join(line.strip() for line in text('sandbox-tools.out').splitlines()[:3])}",
    )
    grant_named = document("pax-grant-as-named.out") or {}
    grant_resolved = document("pax-grant-resolved.out") or {}
    named_text = (text("pax-grant-as-named.out") + text("pax-grant-as-named.err")).strip()
    add(
        "pax_grant_through_home_link",
        "P@x accepts a folder granted as a person would write it, /home/<name>/Documents, although /home is a "
        "link on this system",
        "reached through a link" not in named_text and grant_named.get("error_code") == grant_resolved.get("error_code"),
        f"/home is a link: {status.get('home_is_link')}; $HOME is {status.get('home')}. --grant-read "
        f"{status.get('grant_as_named')}: exit status {codes.get('pax-grant-as-named')}, "
        f"{grant_named.get('error_code') or grant_named.get('outcome')}: "
        f"{' '.join(named_text.split())[:360]} --grant-read {status.get('grant_resolved')}: exit status "
        f"{codes.get('pax-grant-resolved')}, {grant_resolved.get('error_code') or grant_resolved.get('outcome')} "
        "(the provider's address is a closed port: getting that far means the grant was accepted)",
        required=False,
    )

    # ---- what SELinux refused, and what failed, while they ran ----------------------------------
    denials = [" ".join(line.split()) for line in guest.text("components-avc.txt").splitlines() if "avc:" in line]

    def seconds(key: str) -> float | None:
        try:
            return float(status.get(key, ""))
        except ValueError:
            return None

    def during(line: str) -> str:
        """Which part of the run a denial falls in, by the journal's time."""
        found = re.match(r"\[\s*([0-9.]+)\]", line)
        at = float(found.group(1)) if found else None
        spans = {
            "tur-diag status with no new privileges": ("nnp_probe_started_uptime", "nnp_probe_finished_uptime"),
            "the P@x end-to-end test": ("pax_e2e_started_uptime", "pax_e2e_finished_uptime"),
            "P@x's session service test": ("pax_service_started_uptime", "pax_service_finished_uptime"),
            "P@x through local AI": ("local_started_uptime", "local_finished_uptime"),
        }
        for name, (start, end) in spans.items():
            first, last = seconds(start), seconds(end)
            if at is not None and first is not None and last is not None and first - 0.5 <= at <= last + 0.5:
                return name
        return "another part of the run"

    # One denial is understood (TUR-006): tur-diag starts bootc, whose SELinux
    # type has a domain of its own (install_t), from a process that may gain
    # no new privileges. SELinux refuses the change of domain, records it, and
    # bootc runs in the caller's domain. Anything else is not understood.
    known = [
        line
        for line in denials
        if "nnp_transition" in line and 'comm="tur-diag"' in line and ":install_t:" in line and "tclass=process2" in line
    ]
    other = [line for line in denials if line not in known]
    add(
        "selinux_denials",
        "SELinux refused nothing while the programs and the sandbox ran, other than the change of domain "
        "described below",
        not other,
        f"{len(other)} other AVC denials: {' | '.join(line[:400] for line in other[:10]) or 'none'}",
    )
    add(
        "selinux_bootc_under_no_new_privs",
        "no AVC denial is recorded when tur-diag starts bootc from a process that may gain no new privileges, "
        "as in P@x's sandbox",
        not known,
        f"{len(known)} denial(s): "
        + (" | ".join(f"during {during(line)}: {line[:400]}" for line in known[:6]) or "none")
        + f"; bootc's label: {first_line('bootc-label.out') or 'no data'}; tur-diag status with no new privileges: "
        f"exit status {codes.get('diag-status-no-new-privs')}",
        required=False,
    )
    before = {unit.get("unit") for unit in guest.json("failed-units.json") or [] if isinstance(unit, dict)}
    after = {line.split()[0] for line in guest.text("components-failed-units.txt").splitlines() if line.strip()}
    add(
        "no_unit_failed",
        "no systemd unit failed because of them",
        after <= before,
        f"failed before: {sorted(before) or 'none'}; after: {sorted(after) or 'none'}",
    )

    return {
        "ran": ran,
        "run_as": guest.status.get("components_run_as"),
        "user": status.get("user"),
        "selinux_context": context or None,
        "selinux_mode": status.get("selinux_mode"),
        "programs": [
            {"name": row[0], "path": row[1], "exit_status": row[2], "version": row[3]} for row in versions if len(row) >= 4
        ],
        "exit_statuses": codes,
        "diag_bundles": {who: {key: value for key, value in bundle.items() if key != "members"} for who, bundle in bundles.items()},
        "device_inventory_counts": counts,
        "device_findings": {"user": findings(problems), "root": findings(problems_root)},
        "storage_volumes": len(found),
        "pax_service_e2e": {
            "exit_status": codes.get("pax-service-e2e"),
            "passed": service.get("passed"),
            "failed": service.get("failed"),
            "failed_checks": service_failed,
        },
        "pax_local_models": local_runs,
        "pax_e2e": {
            "exit_status": codes.get("pax-e2e"),
            "passed": results.get("passed"),
            "failed": results.get("failed"),
            "scenarios": {name: [sum(1 for item in items if item.get("passed")), len(items)] for name, items in scenarios.items()},
            "failed_checks": failed,
        },
        "avc_denials": denials,
    }


def counter_of(text: str) -> str | None:
    """boot_counter in the output of `grub2-editenv list`, or None."""
    return props(text).get("boot_counter")


def tur_words(cmdline: str) -> dict:
    words = cmdline.split()
    values = {}
    for word in words:
        key, sep, value = word.partition("=")
        if key.startswith("tur."):
            values[key] = value if sep else ""
    return values


def check_boot_health(ev: Evidence, checks: Checks) -> dict:
    """Boot-attempt counting (ADR-0014): steps 1 and 2 of the design's brief.

    Step 2 is required and is the design's go or no-go: a kernel argument
    written as a GRUB variable must survive bootc and OSTree and be expanded
    by GRUB, and GRUB must be able to count in its environment block. Step 1
    is a list of facts the design assumed; those checks are informational.
    """
    first, second, third = (ev.boots[boot] for boot in BOOTS)
    vm, layout = ev.vm, ev.layout
    seen = [boot for boot in (first, second, third) if boot]
    any_boot = seen[0] if seen else first

    def add(ident: str, description: str, passed, detail: str, required: bool = True) -> None:
        checks.add(f"counting.{ident}", f"Boot counting: {description}", passed, detail, required)

    def fact(ident: str, description: str, guest: Guest, name: str, limit: int = 600) -> str:
        text = " ".join(guest.text(name).split())
        checks.add(
            f"measured.{ident}", f"Measured: {description}", bool(text) if guest else None, text[:limit] or "no data", False
        )
        return text

    # ---- step 2: the fragment, the variable, the expansion, the counter ---------
    cfg = any_boot.text("grub.cfg")
    counting, menu = cfg.find("### BEGIN 08_tur_boot_count.cfg ###"), cfg.find("### BEGIN 10_blscfg.cfg ###")
    add(
        "fragment_installed",
        "bootupd wrote the counting fragment into grub.cfg, before the fragment that builds the menu",
        (0 <= counting < menu and bool(layout.get("grub_cfg_counting_fragment_before_menu"))) if (cfg or layout) else None,
        f"in the guest: fragment at offset {counting}, blscfg at {menu}; on the installed disk, read from the host: "
        f"fragments {layout.get('grub_cfg_fragments')}, counting before menu: "
        f"{layout.get('grub_cfg_counting_fragment_before_menu')}",
    )
    options = layout.get("boot_entry_options") or []
    add(
        "variable_in_entry",
        "the GRUB variable survived bootc and OSTree: the loader entry on the installed disk has $tur_boot_args verbatim",
        (bool(options) and all("$tur_boot_args" in option.split() for option in options)) if layout else None,
        f"options: {options or 'no data'}",
    )
    ordinary = []
    for guest in (first, second):
        cmdline = guest.line("cmdline-early.txt") or guest.line("cmdline.txt")
        words = tur_words(cmdline)
        ordinary.append(
            bool(cmdline)
            and words.get("tur.boot_contract") == "1"
            and "tur.boot" not in words
            and "$" not in cmdline
            and counter_of(guest.text("grubenv-early.txt")) is None
        )
    add(
        "ordinary_boot",
        "with no counter set, GRUB expanded the variable to the contract version alone and wrote nothing",
        all(ordinary) if (first and second) else None,
        f"first boot: {first.line('cmdline-early.txt') or 'no data'}; second boot: "
        f"{second.line('cmdline-early.txt') or 'no data'}; boot_counter in the block: "
        f"{counter_of(first.text('grubenv-early.txt'))}, {counter_of(second.text('grubenv-early.txt'))}",
    )
    armed = second.status.get("armed_boot_counter")
    add(
        "set_by_hand",
        "the test set boot_counter=2 in GRUB's environment block before the third boot",
        (armed == "2") if second else None,
        f"boot_counter after grub2-editenv: {armed or 'no data'}; "
        f"{' '.join(second.text('grubenv-armed.txt').split())[:500]}",
    )
    cmdline = third.line("cmdline-early.txt")
    words = tur_words(cmdline)
    early = counter_of(third.text("grubenv-early.txt"))
    add(
        "counted_boot",
        "GRUB decremented and saved the counter before starting the kernel, and told the kernel: "
        "tur.boot=trial tur.boot_tries_left=1",
        (
            words.get("tur.boot_contract") == "1"
            and words.get("tur.boot") == "trial"
            and words.get("tur.boot_tries_left") == "1"
            and "$" not in cmdline
            and early == "1"
        )
        if third
        else None,
        f"/proc/cmdline: {cmdline or 'no data'}; boot_counter read at the start of that boot: {early}",
    )
    console = vm.get("boot_sequence") or []
    add(
        "console_sequence",
        "the kernel command lines printed on the serial console show the same three boots: none, none, trial/1",
        (console == ["none", "none", "trial/1"]) if vm.get("boots") is not None else None,
        f"read by the host from the console: {console or 'no data'}; contract versions: "
        f"{[(boot.get('tur') or {}).get('contract') for boot in vm.get('boots') or []]}",
    )
    final = counter_of(third.text("grubenv-final.txt"))
    late = counter_of(third.text("grubenv-late.txt"))
    removed_by = third.status.get("counter_removed_by") or ("tur-boot-health" if late is None and third else "nobody")
    add(
        "counter_removed",
        "the counter is gone at the end of the third boot, so the disk is left as an ordinary one",
        (final is None) if third.text("grubenv-final.txt") or third else None,
        f"boot_counter late in the third boot: {late}; at the end: {final}; removed by: {removed_by}",
    )
    step2 = [item for item in checks.items if item["id"].startswith("counting.") and item["required"]]
    go = all(item["result"] == "pass" for item in step2)
    checks.add(
        "counting.step2_go",
        "Step 2 of the design's brief, the go or no-go of ADR-0014: every check above passed",
        go if all(item["result"] != "not-run" for item in step2) else None,
        "GO: the argument expansion of design section 5.4 and the counter of section 5.3 work on this image"
        if go
        else "NO-GO: " + ", ".join(item["id"] for item in step2 if item["result"] != "pass"),
    )

    # ---- step 1: what the design assumed -------------------------------------------
    guest = third or second or first
    fact("boot_mount", "the filesystem and options behind /boot/grub2", guest, "measure/findmnt-grub2.txt")
    fact("boot_mount_stack", "everything mounted at /boot", guest, "measure/findmnt-boot.txt")
    fact("boot_mount_units", "the units behind /boot", guest, "measure/boot-mount-show.txt", 900)
    fact("grub_cfg_fragments", "bootupd's fragment markers in grub.cfg", guest, "measure/grub-cfg-fragments.txt")
    fact("unit_enablement", "units the design masks, conditions or relies on", guest, "measure/unit-enablement.txt", 1500)
    fact("boot_complete_target", "what already orders against boot-complete.target", guest, "measure/boot-complete-target.txt", 900)
    fact("is_default", "ostree admin status --is-default", guest, "measure/is-default.txt")
    fact("watchdog_devices", "hardware watchdog devices in the guest", guest, "measure/watchdog-devices.txt")
    fact("manager_watchdog", "PID 1's watchdog settings on an ordinary boot", guest, "measure/manager-watchdog.txt")
    fact("pstore", "the pstore backend and what it holds", guest, "measure/pstore.txt")
    fact("initramfs_watchdog", "watchdog drivers in the initramfs", guest, "measure/initramfs-watchdog.txt")
    fact("kernel_drivers", "the guest kernel's watchdog, panic and fw_cfg drivers", guest, "measure/kernel-drivers.txt", 900)
    fact("tools", "the tools the design calls", guest, "measure/tools.txt", 900)
    block_moves = " ".join(second.text("grubenv-armed.txt").split())
    physical = re.findall(r"0:\s+0\.\.\s*\d+:\s+(\d+)\.\.", second.text("grubenv-armed.txt"))
    checks.add(
        "measured.editenv_block_location",
        "Measured: whether grub2-editenv keeps the environment block in the same disk block when it rewrites it",
        (len(physical) == 2) if second else None,
        (
            f"physical block before {physical[0]}, after {physical[1]}: "
            + ("unchanged" if physical[0] == physical[1] else "MOVED")
            if len(physical) == 2
            else f"filefrag output not understood: {block_moves[:300] or 'no data'}"
        ),
        False,
    )
    st = third.status
    expired = st.get("canary_boot_mount_after_idle")
    checks.add(
        "measured.boot_automount_expiry",
        "Measured: whether a unit that requires /boot is stopped when the idle automount behind /boot expires",
        bool(expired) if third else None,
        f"canary unit with RequiresMountsFor=/boot: {st.get('canary_started', 'no data')} at the start; after "
        f"{st.get('canary_waited_seconds', '?')} s without access boot.mount was {expired or 'no data'} and the canary "
        f"was {st.get('canary_after_idle', 'no data')}",
        False,
    )
    ovmf = ev.out / "host" / "ovmf-files.txt"
    checks.add(
        "measured.ovmf_files",
        "Measured: the firmware images the runner offers (a Secure Boot build is needed for scenario SB)",
        ovmf.exists(),
        " ".join(ovmf.read_text().split())[:900] if ovmf.exists() else "no data",
        False,
    )
    return {
        "console_sequence": console,
        "console_boots": vm.get("boots"),
        "step2_go": go,
        "counter_summary": f"set to {armed} by hand; {early} at the start of the third boot; "
        f"{final} at its end (removed by {removed_by})",
        "kernel_arguments": {boot: ev.boots[boot].line("cmdline-early.txt") or None for boot in BOOTS},
        "grubenv_early": {boot: ev.boots[boot].text("grubenv-early.txt").split() for boot in BOOTS},
        "automount_canary": {key: value for key, value in st.items() if key.startswith("canary_")},
    }


def unit_sections(text: str) -> dict[str, dict[str, str]]:
    """`### unit` sections of KEY=VALUE lines, as the agent writes tur-units.txt."""
    units: dict[str, dict[str, str]] = {}
    current = None
    for line in text.splitlines():
        if line.startswith("### "):
            current = units.setdefault(line[4:].strip(), {})
        elif current is not None and "=" in line:
            key, _, value = line.partition("=")
            current[key.strip()] = value.strip()
    return units


def check_gate(ev: Evidence, checks: Checks) -> dict:
    """The boot health gate on a healthy system (design section 6; steps 6 and 7).

    Every boot of the boot test must pass the gate: the first-run wizard on
    the first boot, the greeter on the second, and the third, which GRUB
    counted, must be marked good by removing the counter.
    """
    gate: dict = {}
    expected = {"first-boot": "normal", "second-boot": "normal", "third-boot": "trial"}
    for boot in BOOTS:
        guest = ev.boots[boot]
        label = boot.replace("-", " ").capitalize()
        status = guest.json("tur-status.json") or {}
        verdict = status.get("verdict") or {}
        results = {item.get("id"): item for item in verdict.get("checks") or []}
        states = {ident: (results.get(ident) or {}).get("state") for ident in ("R1", "R2", "R3", "R4", "R5", "R6", "R7", "R8")}
        classification = (status.get("boot") or {}).get("classification")
        required_ok = all(states[ident] == "succeeded" for ident in ("R1", "R2", "R3", "R4", "R6", "R7", "R8"))
        checks.add(
            f"gate.{boot}",
            f"{label}: the boot health gate passed: R1 to R8 hold, with R5 skipped while no TUR unit is required",
            (
                verdict.get("result") in ("passed", "degraded")
                and required_ok
                and states["R5"] == "skipped"
                and classification == expected[boot]
                and guest.status.get("gate_unit_state") == "active"
                and guest.status.get("mark_good_unit_state") == "active"
            )
            if guest
            else None,
            f"verdict {verdict.get('result')}; classification {classification} (expected {expected[boot]}); checks {states}; "
            f"gate unit {guest.status.get('gate_unit_state', 'no data')}, mark-good unit "
            f"{guest.status.get('mark_good_unit_state', 'no data')}; R7: {(results.get('R7') or {}).get('detail')}; "
            f"degraded by: {[item.get('id') + ': ' + str(item.get('detail')) for item in verdict.get('checks') or [] if not item.get('required') and item.get('state') == 'failed']}",
        )
        units = unit_sections(guest.text("tur-units.txt"))
        graphical = int((units.get("graphical.target") or {}).get("ActiveEnterTimestampMonotonic", "0") or 0)
        finished = int((units.get("tur-boot-health.service") or {}).get("ExecMainExitTimestampMonotonic", "0") or 0)
        started = int((units.get("tur-boot-health.service") or {}).get("ExecMainStartTimestampMonotonic", "0") or 0)
        checks.add(
            f"gate_observes.{boot}",
            f"{label}: the gate observes and holds nothing back: graphical.target was reached while the gate was still running",
            (0 < started < graphical < finished) if units else None,
            f"gate started {started / 1e6:.1f} s, graphical.target {graphical / 1e6:.1f} s, gate finished {finished / 1e6:.1f} s "
            "after kernel start",
        )
        gate[boot] = {
            "verdict": verdict.get("result"),
            "classification": classification,
            "checks": {ident: {"state": item.get("state"), "detail": item.get("detail"), "duration_ms": item.get("duration_ms")} for ident, item in results.items()},
            "gate_started_seconds": round(started / 1e6, 3) if started else None,
            "gate_finished_seconds": round(finished / 1e6, 3) if finished else None,
            "counting": status.get("counting"),
            "watchdog": status.get("watchdog"),
        }
    third = ev.boots["third-boot"]
    history = []
    for line in third.text("boot-health/history.jsonl").splitlines():
        try:
            history.append(json.loads(line).get("event"))
        except ValueError:
            continue
    after = counter_of(third.text("grubenv-after-gate.txt"))
    checks.add(
        "gate.marked_good",
        "Third boot: the counted boot was marked good: tur-boot-health removed the counter after the gate passed",
        (after is None and "marked-good" in history and bool(third.text("grubenv-after-gate.txt") or third.status.get("mark_good_unit_state")))
        if third
        else None,
        f"boot_counter at the start of the boot: {counter_of(third.text('grubenv-early.txt'))}; after the gate: {after}; "
        f"history: {history}",
    )
    checks.add(
        "gate.arm_unit_survives_idle",
        "Third boot: the unit that arms a trial at shutdown is still active after the automount behind /boot expired",
        (third.status.get("arm_unit_after_idle") == "active" and third.status.get("canary_after_idle") in ("inactive", "failed"))
        if third.status.get("arm_unit_after_idle")
        else None,
        f"tur-boot-arm.service: {third.status.get('arm_unit_after_idle', 'no data')}; a canary unit that requires /boot: "
        f"{third.status.get('canary_after_idle', 'no data')} after boot.mount became "
        f"{third.status.get('canary_boot_mount_after_idle', 'no data')}",
    )
    verify = " ".join((third or ev.boots["second-boot"]).text("systemd-analyze-verify.txt").split())
    checks.add(
        "gate.unit_graph",
        "systemd-analyze verify on the boot-health units, in the booted system",
        (not verify) if third else None,
        verify[:900] or "no complaints",
        required=False,
    )
    return gate


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
    first, second, third = (ev.boots[boot] for boot in BOOTS)
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
        "The serial console was captured from firmware to the test agent, for all three boots",
        bool(ev.serial) and banners >= 3 and starts >= 3,
        f"{len(ev.serial)} bytes; kernel banners: {banners}; agent start markers: {starts}",
    )
    boot_ids = {guest.status.get("boot_id") for guest in (first, second, third) if guest}
    checks.add(
        "three_boots",
        "The same disk booted three times: as installed, after first-run setup, and with GRUB's boot counter set",
        (len(boot_ids) == 3 and first.status.get("second_boot_prepared") == "yes") if (first or second) else None,
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

    binaries = ev.layout.get("esp_efi_binaries") or {}
    chain = {name: info for name, info in binaries.items() if name.lower().endswith(("/shimx64.efi", "/grubx64.efi"))}
    checks.add(
        "boot_chain_signed",
        "shim and GRUB on the EFI system partition carry Authenticode signatures (presence only; not verified by firmware)",
        (len(chain) >= 2 and all(info.get("signed") for info in chain.values())) if binaries else None,
        "; ".join(
            f"{name}: {'signed' if info.get('signed') else 'NOT signed'}"
            f" ({', '.join(info.get('names_in_certificates') or []) or 'no names read'})"
            for name, info in sorted(chain.items())
        )
        or "no data",
        required=False,
    )

    boots = [check_boot(ev, boot, checks, args, allowed) for boot in BOOTS]
    boot_health = check_boot_health(ev, checks)
    boot_health["gate"] = check_gate(ev, checks)
    components = check_components(ev, checks, args)

    checks.add(
        "clean_shutdown",
        "The guest powered off when asked",
        (vm.get("shutdown") == "clean") if vm.get("shutdown") else None,
        f"shutdown: {vm.get('shutdown', 'not attempted')}",
        required=False,
    )

    # ---- report ------------------------------------------------------------------------
    tur_release = props(first.text("tur-release") or second.text("tur-release"))
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
        "task": "TUR-003",
        "tasks": ["TUR-003", "TUR-005", "TUR-006"],
        "result": "pass" if checks.passed() else "fail",
        "build_id": boots[0]["build_id"] or boots[1]["build_id"] or boots[2]["build_id"],
        "expected_build_id": args.expected_build_id or None,
        "source_revision": tur_release.get("TUR_SOURCE_REVISION"),
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
        "boot_health": boot_health,
        "components": components,
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
        f"### TUR boot test: {report['result'].upper()}",
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

    local_summary = ", ".join(
        f"{name} {run.get('serve')}" for name, run in components["pax_local_models"].items()
    ) or "not run"
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
        ("Kind of each boot, read from the console", ", ".join(boot_health["console_sequence"]) or "no data"),
        ("GRUB boot counter", boot_health["counter_summary"]),
        (
            "TUR programs (second boot)",
            f"run as {components['run_as']}, {components['selinux_context']}, SELinux {components['selinux_mode']}; "
            f"{len(components['programs'])} programs; P@x end to end: {components['pax_e2e']['passed']} passed, "
            f"{components['pax_e2e']['failed']} failed; session service: {components['pax_service_e2e']['passed']} "
            f"passed, {components['pax_service_e2e']['failed']} failed; local model: "
            f"{local_summary}; AVC denials while they ran: {len(components['avc_denials'])}",
        ),
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
