#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Look inside a TUR disk image from the host, read-only.

    sudo tests/boot/inspect-disk.py installed  DISK OUT_DIR
    sudo tests/boot/inspect-disk.py postmortem DISK OUT_DIR

"installed" runs after `bootc install` and before the first boot. It records the
partition table, the content of the EFI system partition, the boot loader
entries and the install record, so that the layout and bootloader are measured
from outside the guest and do not depend on the guest booting.

"postmortem" runs after the virtual machine has stopped, whatever happened. It
copies the persistent journal and anything the test agent left on disk, so a
boot that never reported still leaves evidence. It is best effort and always
exits 0.

The disk is attached through a read-only loop device and nothing is replayed
or written. Needs root. Run in CI only.
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import shutil
import struct
import subprocess
import sys
import tempfile

ESP = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"
BIOS_BOOT = "21686148-6449-6e6f-744e-656564454649"
ROOT_X86_64 = "4f68bce3-e8cd-4db1-96e7-fbcaf984b709"
XBOOTLDR = "bc13c2ff-59e6-4262-a352-b275fd6f7172"
ROLES = {ESP: "esp", BIOS_BOOT: "bios-boot", ROOT_X86_64: "root", XBOOTLDR: "xbootldr"}
LSBLK_COLUMNS = "NAME,PATH,SIZE,TYPE,FSTYPE,LABEL,PARTLABEL,PARTTYPE,PARTTYPENAME,UUID,PARTUUID"
# Read-only mount options that do not replay a journal, per filesystem.
MOUNT_OPTIONS = {"ext4": "ro,noload", "xfs": "ro,norecovery", "btrfs": "ro", "vfat": "ro"}


def run(*cmd: str, check: bool = True) -> str:
    result = subprocess.run(cmd, capture_output=True, text=True)
    if check and result.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd)} failed ({result.returncode}): {result.stderr.strip()}")
    return result.stdout


class Disk:
    """A disk image attached read-only, with its partitions described."""

    def __init__(self, path: str):
        self.path = path
        self.loop = ""
        self.mounted: list[str] = []
        self.tmp = tempfile.mkdtemp(prefix="tur-inspect-")
        self.partitions: list[dict] = []
        self.table: dict = {}
        self.lsblk: dict = {}

    def __enter__(self) -> "Disk":
        self.loop = run("losetup", "--find", "--show", "--partscan", "--read-only", self.path).strip()
        run("udevadm", "settle", "--timeout=30", check=False)
        self.table = json.loads(run("sfdisk", "--json", self.loop))
        self.lsblk = json.loads(run("lsblk", "--json", "--bytes", "-o", LSBLK_COLUMNS, self.loop))
        by_path = {}
        for device in self.lsblk.get("blockdevices", []):
            for child in device.get("children", []) or []:
                by_path[child.get("path")] = child
        sector = int(self.table["partitiontable"].get("sectorsize", 512))
        for number, part in enumerate(self.table["partitiontable"].get("partitions", []), start=1):
            node = part["node"]
            seen = by_path.get(node, {})
            fstype = seen.get("fstype") or run("blkid", "-o", "value", "-s", "TYPE", node, check=False).strip() or None
            type_guid = part["type"].lower()
            self.partitions.append(
                {
                    "number": number,
                    "node": node,
                    "name": part.get("name"),
                    "role": ROLES.get(type_guid, "other"),
                    "type_guid": type_guid,
                    "type_name": seen.get("parttypename"),
                    "start_bytes": int(part["start"]) * sector,
                    "size_bytes": int(part["size"]) * sector,
                    "filesystem": fstype,
                    "filesystem_label": seen.get("label"),
                    "filesystem_uuid": seen.get("uuid"),
                    "partition_uuid": (part.get("uuid") or "").lower() or None,
                }
            )
        return self

    def __exit__(self, *exc) -> None:
        for target in reversed(self.mounted):
            run("umount", target, check=False)
        if self.loop:
            run("losetup", "--detach", self.loop, check=False)
        shutil.rmtree(self.tmp, ignore_errors=True)

    def find(self, role: str) -> dict | None:
        return next((p for p in self.partitions if p["role"] == role), None)

    def mount(self, part: dict) -> pathlib.Path:
        target = pathlib.Path(self.tmp) / f"p{part['number']}"
        target.mkdir(exist_ok=True)
        options = MOUNT_OPTIONS.get(part["filesystem"] or "", "ro")
        run("mount", "-o", options, part["node"], str(target))
        self.mounted.append(str(target))
        return target

    def layout(self) -> dict:
        table = self.table["partitiontable"]
        return {
            "label": table.get("label"),
            "sector_size": table.get("sectorsize"),
            "disk_bytes": os.path.getsize(self.path),
            "partitions": [{k: v for k, v in p.items() if k != "node"} for p in self.partitions],
        }


def listing(root: pathlib.Path) -> list[tuple[int, str]]:
    """Every file under a directory, with its size."""
    found = []
    for path in sorted(root.rglob("*")):
        if path.is_file() and not path.is_symlink():
            found.append((path.stat().st_size, f"/{path.relative_to(root)}"))
    return found


def authenticode(path: pathlib.Path) -> dict:
    """Whether an EFI binary carries an Authenticode signature, and names in it.

    Reads the PE certificate table. This shows that a signature is attached and
    which organisations its certificates name. It does not verify the signature
    or the chain: only firmware with Secure Boot enforced does that.
    """
    data = path.read_bytes()
    try:
        pe = struct.unpack_from("<I", data, 0x3C)[0]
        if data[pe:pe + 4] != b"PE\0\0":
            return {"signed": False, "note": "not a PE file"}
        optional = pe + 24
        magic = struct.unpack_from("<H", data, optional)[0]
        directories = optional + (112 if magic == 0x20B else 96)
        offset, size = struct.unpack_from("<II", data, directories + 4 * 8)
    except struct.error:
        return {"signed": False, "note": "truncated PE header"}
    table = data[offset:offset + size] if size else b""
    names = sorted(
        {
            name.decode().strip()
            for name in re.findall(rb"[A-Za-z][A-Za-z0-9 .,()-]{6,60}", table)
            if any(word in name for word in (b"Fedora", b"Microsoft", b"Red Hat"))
        }
    )
    return {"signed": size > 0, "signature_bytes": size, "names_in_certificates": names}


def read_text(path: pathlib.Path) -> str:
    return path.read_bytes().decode("utf-8", errors="replace")


def inspect_esp(disk: Disk, host: pathlib.Path, layout: dict) -> None:
    esp = disk.find("esp")
    if not esp:
        raise RuntimeError("no EFI system partition found")
    mnt = disk.mount(esp)
    files = listing(mnt)
    text = "".join(f"{size:>10}  {name}\n" for size, name in files)
    (host / "esp-files.txt").write_text(text)
    print("EFI system partition:")
    print(text)
    layout["esp_files"] = [name for _size, name in files]
    layout["esp_efi_binaries"] = {
        name: authenticode(mnt / name.lstrip("/")) for _size, name in files if name.lower().endswith(".efi")
    }
    print("EFI binaries and their signatures:")
    print(json.dumps(layout["esp_efi_binaries"], indent=2))
    configs = []
    for cfg in sorted(mnt.rglob("*")):
        if cfg.is_file() and cfg.name.lower() in ("grub.cfg", "bootuuid.cfg", "loader.conf"):
            configs.append(f"### /{cfg.relative_to(mnt)}\n{read_text(cfg)}\n")
    (host / "esp-configs.txt").write_text("\n".join(configs))
    print("".join(configs))


def inspect_root(disk: Disk, host: pathlib.Path, layout: dict) -> None:
    root = disk.find("root")
    if not root:
        raise RuntimeError("no x86-64 root partition found")
    mnt = disk.mount(root)
    stat = os.statvfs(mnt)
    layout["root_used_bytes"] = (stat.f_blocks - stat.f_bfree) * stat.f_frsize
    boot = mnt / "boot"
    xbootldr = disk.find("xbootldr")
    layout["separate_boot_partition"] = bool(xbootldr)
    if xbootldr:
        boot = disk.mount(xbootldr)
    entries = []
    for entry in sorted(boot.glob("loader/entries/*.conf")):
        entries.append(f"### /boot/{entry.relative_to(boot)}\n{read_text(entry)}\n")
    (host / "boot-entries.txt").write_text("\n".join(entries))
    layout["boot_entries"] = len(entries)
    print("Boot loader entries:")
    print("".join(entries) or "(none found)")
    # GRUB's own configuration and environment: where boot counting would live.
    grub = []
    for name in ("grub2/grub.cfg", "grub2/grubenv", "grub2/bootuuid.cfg", "loader/loader.conf"):
        if (boot / name).is_file():
            grub.append(f"### /boot/{name}\n{read_text(boot / name)}\n")
    (host / "boot-grub-config.txt").write_text("\n".join(grub))
    layout["grub_config_mentions_boot_counter"] = any("boot_counter" in text for text in grub)
    # Boot-attempt counting (ADR-0014): the fragment bootupd wrote into grub.cfg,
    # and the GRUB variable OSTree copied into the loader entries.
    cfg = read_text(boot / "grub2" / "grub.cfg") if (boot / "grub2" / "grub.cfg").is_file() else ""
    counting, menu = cfg.find("### BEGIN 08_tur_boot_count.cfg ###"), cfg.find("### BEGIN 10_blscfg.cfg ###")
    layout["grub_cfg_fragments"] = re.findall(r"^### BEGIN (\S+) ###$", cfg, re.M)
    layout["grub_cfg_counting_fragment_before_menu"] = 0 <= counting < menu
    layout["boot_entry_options"] = [
        line.split(None, 1)[1] for text in entries for line in text.splitlines() if line.startswith("options ")
    ]
    block = boot / "grub2" / "grubenv"
    layout["grubenv_bytes"] = block.stat().st_size if block.is_file() else None
    listed = []
    for name in ("", "boot", "boot/grub2", "boot/loader", "ostree", "ostree/deploy"):
        listed.append(f"### /{name}\n" + run("ls", "-la", str(mnt / name), check=False))
    for deploy in sorted(mnt.glob("ostree/deploy/*/deploy")):
        listed.append(f"### /{deploy.relative_to(mnt)}\n" + run("ls", "-la", str(deploy), check=False))
    (host / "root-listing.txt").write_text("\n".join(listed))
    print("\n".join(listed))
    aleph = mnt / ".bootc-aleph.json"
    if aleph.exists():
        (host / "bootc-aleph.json").write_text(read_text(aleph))
        print(".bootc-aleph.json:")
        print(read_text(aleph))


def installed(disk_path: str, out: pathlib.Path) -> int:
    host = out / "host"
    host.mkdir(parents=True, exist_ok=True)
    with Disk(disk_path) as disk:
        layout = disk.layout()
        print(json.dumps(layout, indent=2))
        print(run("lsblk", "-o", LSBLK_COLUMNS, disk.loop))
        errors = []
        for section in (inspect_esp, inspect_root):
            try:
                section(disk, host, layout)
            except Exception as error:  # record it; the report decides what it means
                errors.append(f"{section.__name__}: {error}")
                print(f"inspection problem: {section.__name__}: {error}")
        layout["inspection_errors"] = errors
        (host / "disk-layout.json").write_text(json.dumps(layout, indent=2) + "\n")
    return 0


def boot_state(mnt: pathlib.Path, out: pathlib.Path) -> None:
    """What the bootloader will see next: the environment block, the loader
    entries and the deployments, as they are on the disk right now."""
    boot = mnt / "boot"
    parts = []
    block = boot / "grub2" / "grubenv"
    if block.is_file():
        data = block.read_bytes()
        text = data.decode("utf-8", errors="replace").rstrip("#\n")
        valid = len(data) == 1024 and data.startswith(b"# GRUB Environment Block")
        parts.append(f"### /boot/grub2/grubenv ({len(data)} bytes, valid block: {valid})\n{text}\n")
    else:
        parts.append("### /boot/grub2/grubenv is absent\n")
    loader = boot / "loader"
    parts.append(f"### /boot/loader -> {os.readlink(loader) if loader.is_symlink() else '(not a link)'}\n")
    for entry in sorted(boot.glob("loader*/entries/*.conf")):
        parts.append(f"### /boot/{entry.relative_to(boot)}\n{read_text(entry)}\n")
    parts.append("### /boot/ostree\n" + run("ls", "-la", str(boot / "ostree"), check=False))
    for deploy in sorted(mnt.glob("ostree/deploy/*/deploy")):
        parts.append(f"### /{deploy.relative_to(mnt)}\n" + run("ls", "-la", str(deploy), check=False))
    text = "\n".join(parts)
    (out / "boot-state-disk.txt").write_text(text)
    print("postmortem: boot state on the disk:")
    print(text)


def postmortem(disk_path: str, out: pathlib.Path) -> int:
    try:
        with Disk(disk_path) as disk:
            root = disk.find("root")
            if not root:
                print("postmortem: no root partition found")
                return 0
            mnt = disk.mount(root)
            boot_state(mnt, out)
            for state in sorted(mnt.glob("ostree/deploy/*/var")):
                health = state / "lib" / "tur" / "boot-health"
                if health.is_dir():
                    target = out / "boot-health-disk"
                    shutil.copytree(health, target, dirs_exist_ok=True, ignore=shutil.ignore_patterns("*.zst"))
                    print(f"postmortem: copied {health.relative_to(mnt)}")
                journal = state / "log" / "journal"
                if journal.is_dir():
                    target = out / "journal-disk"
                    shutil.copytree(journal, target, dirs_exist_ok=True)
                    size = sum(f.stat().st_size for f in target.rglob("*") if f.is_file())
                    print(f"postmortem: copied the persistent journal from {journal.relative_to(mnt)} ({size} bytes)")
                    text = subprocess.run(
                        ["journalctl", "--directory", str(journal), "--no-pager", "-o", "short-monotonic"],
                        capture_output=True,
                        text=True,
                        errors="replace",
                    )
                    if text.returncode == 0 and text.stdout.strip():
                        (out / "journal-disk.txt").write_text(text.stdout)
                        print(f"postmortem: rendered it as text ({len(text.stdout.splitlines())} lines)")
                    else:
                        print(f"postmortem: the host journalctl could not render it: {text.stderr.strip()[:300]}")
                else:
                    print(f"postmortem: no persistent journal under {state.relative_to(mnt)}")
                report = state / "lib" / "tur-boot-test" / "report"
                if report.is_dir():
                    shutil.copytree(report, out / "guest-from-disk", dirs_exist_ok=True)
                    print("postmortem: copied the agent's on-disk report")
    except Exception as error:  # best effort by design
        print(f"postmortem: failed: {error}")
    return 0


def give_back(path: pathlib.Path) -> None:
    """Hand files created as root back to the invoking user."""
    uid, gid = os.environ.get("SUDO_UID"), os.environ.get("SUDO_GID")
    if not uid or not gid:
        return
    for directory, _dirs, files in os.walk(path):
        os.chown(directory, int(uid), int(gid))
        for name in files:
            os.chown(os.path.join(directory, name), int(uid), int(gid), follow_symlinks=False)


def main() -> int:
    if len(sys.argv) != 4 or sys.argv[1] not in ("installed", "postmortem"):
        print(__doc__, file=sys.stderr)
        return 2
    mode, disk_path, out = sys.argv[1], sys.argv[2], pathlib.Path(sys.argv[3])
    out.mkdir(parents=True, exist_ok=True)
    try:
        return installed(disk_path, out) if mode == "installed" else postmortem(disk_path, out)
    finally:
        give_back(out)


if __name__ == "__main__":
    sys.exit(main())
