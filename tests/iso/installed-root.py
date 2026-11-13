#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Look at the root filesystem of a disk the installer medium installed, from
the host, read-only; or, for an experiment, give it ext4's casefold feature.

    sudo tests/iso/installed-root.py inspect DISK OUT_DIR
    sudo tests/iso/installed-root.py casefold DISK OUT_DIR

inspect writes OUT_DIR/installed-root.json and copies what the installer kept
on the disk (/var/log/tur-installer/<session>/) to OUT_DIR/installer-log/:

  - the root filesystem's features (dumpe2fs -h), and whether casefold is
    among them. TUR's installer does not give the root casefold: GRUB, which
    reads /boot from that filesystem at every start, refuses an ext4
    filesystem with a feature it does not know (ADR-0018, amendment of
    13 November 2026; issue 81);
  - the installer's log kept on the disk (specification 28.3), with its
    SELinux label.

casefold runs `tune2fs -O casefold -E encoding=utf8` on the root partition
(unmounted), for
the informational run that starts such a disk to see what GRUB does. It is
never part of an installation.

Run in CI only, as root, on disposable disk images (AGENTS.md).
"""

from __future__ import annotations

import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

ROOT_X86_64 = "4f68bce3-e8cd-4db1-96e7-fbcaf984b709"


def run(*command: str, check: bool = True) -> str:
    done = subprocess.run(command, capture_output=True, text=True)
    if check and done.returncode != 0:
        raise RuntimeError(f"{' '.join(command)}: status {done.returncode}: {done.stderr.strip()}")
    return done.stdout + (done.stderr if not check else "")


def root_partition(loop: str) -> str:
    table = json.loads(run("sfdisk", "--json", loop))["partitiontable"]["partitions"]
    node = next((part["node"] for part in table if part["type"].lower() == ROOT_X86_64), None)
    if not node:
        raise RuntimeError(f"no x86-64 root partition in {table}")
    for _ in range(30):
        if os.path.exists(node):
            return node
        run("partx", "--add", loop, check=False)
        run("udevadm", "settle", "--timeout=10", check=False)
    raise RuntimeError(f"{node} did not appear")


def selinux_label(path: pathlib.Path) -> str | None:
    try:
        done = subprocess.run(["getfattr", "--only-values", "-h", "-n", "security.selinux", str(path)],
                              capture_output=True)
    except OSError:
        return None
    return done.stdout.decode(errors="replace").strip("\x00\n ") or None if done.returncode == 0 else None


def features(device: str) -> list[str]:
    for line in run("dumpe2fs", "-h", device).splitlines():
        if line.startswith("Filesystem features:"):
            return line.split(":", 1)[1].split()
    return []


def inspect(disk: str, out: pathlib.Path) -> int:
    facts: dict = {"disk": disk}
    loop = run("losetup", "--find", "--show", "--partscan", "--read-only", disk).strip()
    mnt = pathlib.Path(tempfile.mkdtemp(prefix="tur-installed-root-"))
    mounted = False
    try:
        root = root_partition(loop)
        facts["root_partition"] = root
        facts["features"] = features(root)
        facts["casefold"] = "casefold" in facts["features"]
        run("mount", "-o", "ro,noload", root, str(mnt))
        mounted = True
        facts["state_root_var"] = {
            str(var.relative_to(mnt)): sorted(child.name for child in var.iterdir())
            for var in sorted(mnt.glob("ostree/deploy/*/var")) if var.is_dir()
        }
        facts["filesystem_state"] = next(
            (line.split(":", 1)[1].strip() for line in run("dumpe2fs", "-h", root).splitlines()
             if line.startswith("Filesystem state:")), None)
        kept = sorted(mnt.glob("ostree/deploy/*/var/log/tur-installer/*"))
        facts["installer_log_directories"] = [str(path.relative_to(mnt)) for path in kept]
        if kept:
            source = kept[-1]
            target = out / "installer-log"
            shutil.rmtree(target, ignore_errors=True)
            target.mkdir(parents=True)
            files = []
            for path in sorted(source.iterdir()):
                files.append({"name": path.name, "bytes": path.stat().st_size,
                              "mode": oct(path.stat().st_mode & 0o7777), "selinux": selinux_label(path)})
                if path.is_file():
                    shutil.copyfile(path, target / path.name)
            facts["installer_log_files"] = files
            facts["installer_log_directory_selinux"] = selinux_label(source)
            engine_lines = 0
            log = source / "installer.log"
            if log.exists():
                engine_lines = sum(1 for line in log.read_text(errors="replace").splitlines() if line.startswith("ENGINE> "))
            facts["engine_lines_in_kept_log"] = engine_lines
    except (RuntimeError, OSError, ValueError) as error:
        facts["error"] = str(error)
    finally:
        if mounted:
            run("umount", str(mnt), check=False)
        run("losetup", "--detach", loop, check=False)
        shutil.rmtree(mnt, ignore_errors=True)
    out.mkdir(parents=True, exist_ok=True)
    (out / "installed-root.json").write_text(json.dumps(facts, indent=2) + "\n")
    print(json.dumps(facts, indent=2))
    for path in out.rglob("*"):
        os.chmod(path, 0o755 if path.is_dir() else 0o644)
    return 0 if "error" not in facts else 1


def casefold(disk: str, out: pathlib.Path) -> int:
    facts: dict = {"disk": disk}
    loop = run("losetup", "--find", "--show", "--partscan", disk).strip()
    try:
        root = root_partition(loop)
        facts["features_before"] = features(root)
        done = subprocess.run(["tune2fs", "-O", "casefold", "-E", "encoding=utf8", root], capture_output=True, text=True)
        facts["tune2fs"] = {"status": done.returncode, "output": (done.stdout + done.stderr).strip()}
        check = subprocess.run(["e2fsck", "-fn", root], capture_output=True, text=True)
        facts["e2fsck_after"] = {"status": check.returncode, "output": (check.stdout + check.stderr).strip()[-2000:]}
        facts["features_after"] = features(root)
        facts["casefold"] = "casefold" in facts["features_after"]
    except (RuntimeError, OSError, ValueError) as error:
        facts["error"] = str(error)
    finally:
        run("losetup", "--detach", loop, check=False)
    out.mkdir(parents=True, exist_ok=True)
    (out / "casefold.json").write_text(json.dumps(facts, indent=2) + "\n")
    os.chmod(out / "casefold.json", 0o644)
    print(json.dumps(facts, indent=2))
    return 0 if facts.get("casefold") else 1


def main() -> int:
    if len(sys.argv) != 4 or sys.argv[1] not in ("inspect", "casefold"):
        print(__doc__, file=sys.stderr)
        return 2
    what, disk, out = sys.argv[1], sys.argv[2], pathlib.Path(sys.argv[3])
    return inspect(disk, out) if what == "inspect" else casefold(disk, out)


if __name__ == "__main__":
    sys.exit(main())
