#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Measure installer/iso/rootfs/usr/lib/tur-installer/ntfs-shrink.sh (issue
64, step 2; ADR-0029) on crafted NTFS images. No Windows is involved: every
volume is made by ntfs-3g's mkfs.ntfs on a disposable disk image.

    sudo tests/iso/ntfs-shrink-test.py OUT_DIR [WORK_DIR]

Each disk image is GPT: a 100 MiB FAT "EFI system partition" (1), an NTFS
"Windows" partition (2), free space after it. Scenarios:

  dry-run       ntfsresize --no-action changes no byte of the partition
  shrink        a volume whose files lie far from its start (40 files of
                128 MiB written, the first 30 deleted) is shrunk to its
                smallest size plus 512 MiB: clusters move. Afterwards every
                remaining file has its SHA-256, the volume is the new size,
                partition 2 is smaller with the same start, type, UUID and
                name, and partition 1 is byte-identical
  dirty         the shrunk volume, which ntfsresize marks for a check by
                Windows, is refused (TUR-INSTALL-SHRINK-004), unchanged
  bitlocker     a volume whose first sector carries "-FVE-FS-" is refused
                (TUR-INSTALL-SHRINK-001), unchanged
  hibernated    a volume holding hiberfil.sys that begins "HIBR" (Windows
                hibernated or shut down with Fast Startup) is refused
                (TUR-INSTALL-SHRINK-003), unchanged
  too-full      a volume too full to keep the margin is refused
                (TUR-INSTALL-SHRINK-005), unchanged
  interrupted   measured, not judged: ntfsresize killed (SIGKILL) 0.5, 1, 2
                and 4 s into the shrink; what ntfsresize --info and ntfsfix
                -n then say, and how many files still have their SHA-256.
                A killed process is not a power cut: the kernel still writes
                what the process had written

The margin is lowered for these small volumes (NTFS_MARGIN_BYTES=256 MiB;
the library's default is 16 GiB). Writes OUT_DIR/ntfs-shrink.json and
OUT_DIR/summary.md; exits 1 if a judged scenario failed. Needs root, ntfs-3g,
sfdisk, losetup, mkfs.vfat. Run in CI only (ADR-0002): disposable images.
"""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import time

LIBRARY = pathlib.Path(__file__).resolve().parents[2] / "installer/iso/rootfs/usr/lib/tur-installer/ntfs-shrink.sh"
MIB = 1024 * 1024
MARGIN = 256 * MIB


def run(*command: str, check: bool = True, **kwargs) -> subprocess.CompletedProcess:
    return subprocess.run(command, capture_output=True, text=True, check=check, **kwargs)


def sha(path: pathlib.Path | str, start: int = 0, size: int | None = None) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        stream.seek(start)
        left = size if size is not None else 1 << 62
        while left > 0:
            chunk = stream.read(min(left, 1 << 22))
            if not chunk:
                break
            digest.update(chunk)
            left -= len(chunk)
    return digest.hexdigest()


class Image:
    """A disposable disk image attached to a loop device."""

    def __init__(self, path: pathlib.Path, ntfs_mib: int, total_mib: int):
        self.path = path
        path.unlink(missing_ok=True)
        run("truncate", "-s", f"{total_mib}M", str(path))
        table = (f"label: gpt\nstart=2048, size={100 * MIB // 512}, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, "
                 f"name=\"EFI system partition\"\nsize={ntfs_mib * MIB // 512}, "
                 f"type=EBD0A0A2-B9E5-4433-87C0-68B6B72699C7, name=\"Basic data partition\"\n")
        run("sfdisk", "--quiet", str(path), input=table)
        self.loop = ""
        self.attach()
        run("mkfs.vfat", "-F", "32", "-n", "SYSTEM", self.part(1))
        run("mkfs.ntfs", "--quick", "--label", "Windows", self.part(2))

    def attach(self) -> None:
        self.loop = run("losetup", "--find", "--show", "--partscan", str(self.path)).stdout.strip()
        run("udevadm", "settle", check=False)

    def detach(self) -> None:
        if self.loop:
            run("losetup", "--detach", self.loop, check=False)
            self.loop = ""

    def part(self, number: int) -> str:
        return f"{self.loop}p{number}"

    def table(self) -> dict:
        return json.loads(run("sfdisk", "--json", self.loop).stdout)["partitiontable"]

    def partition_sha(self, number: int) -> str:
        return sha(self.part(number))


def mount(part: str, where: pathlib.Path, readonly: bool = False) -> None:
    where.mkdir(parents=True, exist_ok=True)
    run("mount", "-t", "ntfs-3g", "-o", "ro" if readonly else "rw", part, str(where))


def umount(where: pathlib.Path) -> None:
    run("umount", str(where), check=False)


def library(function: str, *arguments: str) -> dict:
    """Call a function of the library in a fresh bash; return its variables."""
    script = (f'. "{LIBRARY}"; {function} "$@"; status=$?; '
              'printf "status=%s\\nn_state=%s\\nn_code=%s\\nn_size=%s\\nn_min=%s\\ns_code=%s\\n" '
              '"$status" "$n_state" "$n_code" "$n_size" "$n_min" "${s_code:-}"')
    done = subprocess.run(["bash", "-c", script, "_", *arguments], capture_output=True, text=True,
                          env=dict(os.environ, NTFS_MARGIN_BYTES=str(MARGIN)))
    values = dict(line.split("=", 1) for line in done.stdout.splitlines() if "=" in line)
    values["state_lines"] = [line for line in done.stderr.splitlines() if line.startswith("TUR-INSTALLER:")]
    return values


def fill_far(image: Image, work: pathlib.Path) -> dict[str, str]:
    """40 files of 128 MiB, then the first 30 deleted: what remains lies far
    from the volume's start. Returns name -> SHA-256 of what remains."""
    where = work / "mnt"
    mount(image.part(2), where)
    hashes = {}
    try:
        for number in range(40):
            target = where / f"file{number:02d}.bin"
            with open(target, "wb") as stream:
                stream.write(os.urandom(128 * MIB))
            if number >= 30:
                hashes[target.name] = sha(target)
        for number in range(30):
            (where / f"file{number:02d}.bin").unlink()
        small = where / "keep-me.txt"
        small.write_text("This file stands for the data on a Windows partition.\n")
        hashes[small.name] = sha(small)
    finally:
        umount(where)
    return hashes


def file_hashes(image: Image, names: list[str]) -> dict[str, str | None]:
    found = {}
    for name in names:
        done = subprocess.run(["ntfscat", image.part(2), name], capture_output=True)
        found[name] = hashlib.sha256(done.stdout).hexdigest() if done.returncode == 0 else None
    return found


def volume_bytes(image: Image) -> int | None:
    info = run("ntfsresize", "--info", "--force", "--no-progress-bar", image.part(2), check=False).stdout
    for line in info.splitlines():
        if line.startswith("Current volume size"):
            return int(line.split(":")[1].split()[0])
    return None


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    out = pathlib.Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    work = pathlib.Path(sys.argv[2] if len(sys.argv) > 2 else tempfile.mkdtemp(prefix="tur-ntfs-"))
    work.mkdir(parents=True, exist_ok=True)
    results: dict = {"margin_bytes": MARGIN, "ntfs_3g": run("dpkg-query", "-W", "-f=${Version}", "ntfs-3g", check=False).stdout,
                     "scenarios": {}}
    checks: list[tuple[str, str, bool | None, str]] = []

    def check(scenario: str, what: str, ok: bool | None, detail: str = "") -> None:
        checks.append((scenario, what, ok, detail))
        print(f"[{scenario}] {what}: {'ok' if ok else ('FAIL' if ok is False else 'recorded')} {detail}", flush=True)

    # --- the fragmented volume: dry run, shrink, dirty ------------------------------
    base = Image(work / "far.raw", ntfs_mib=6144, total_mib=8192)
    try:
        hashes = fill_far(base, work)
        look = library("ntfs_look", base.part(2))
        results["scenarios"]["look"] = look
        check("shrink", "the clean volume may be shrunk", look.get("n_state") == "ok", str(look))
        before_esp = base.partition_sha(1)
        before_ntfs = base.partition_sha(2)
        before_table = base.table()
        new = int(look.get("n_min") or 0) + 512 * MIB
        new -= new % MIB
        started = time.monotonic()
        dry = run("ntfsresize", "--no-action", "--no-progress-bar", "--size", str(new), base.part(2), check=False)
        results["scenarios"]["dry-run"] = {"status": dry.returncode, "seconds": round(time.monotonic() - started, 2),
                                           "output": dry.stdout[-1500:]}
        check("dry-run", "ntfsresize --no-action changes no byte", base.partition_sha(2) == before_ntfs,
              f"status {dry.returncode}")
        run("cp", "--sparse=always", str(base.path), str(work / "far-pristine.raw"))
        started = time.monotonic()
        shrink = library("ntfs_shrink", base.loop, "2", base.part(2), str(new))
        seconds = round(time.monotonic() - started, 2)
        base.detach()
        base.attach()
        after_table = base.table()
        results["scenarios"]["shrink"] = {"from": look.get("n_size"), "min": look.get("n_min"), "to": new,
                                          "seconds": seconds, "result": shrink}
        check("shrink", "the library reports the shrink done", shrink.get("status") == "0", str(shrink.get("state_lines")))
        now = file_hashes(base, list(hashes))
        check("shrink", "every remaining file has its SHA-256", now == hashes,
              f"{sum(1 for k in hashes if now.get(k) == hashes[k])} of {len(hashes)}; moved from beyond "
              f"{new // MIB} MiB")
        size = volume_bytes(base)
        check("shrink", "the volume is the new size", size is not None and size <= new, f"{size} bytes, asked {new}")
        old2, new2 = before_table["partitions"][1], after_table["partitions"][1]
        same = {k: old2.get(k) == new2.get(k) for k in ("start", "type", "uuid", "name")}
        check("shrink", "partition 2: same start, type, UUID and name; smaller",
              all(same.values()) and int(new2["size"]) < int(old2["size"]) and int(new2["size"]) * 512 >= new,
              f"{old2['size']} -> {new2['size']} sectors; {same}")
        check("shrink", "partition 1 byte-identical", base.partition_sha(1) == before_esp)
        fix = run("ntfsfix", "--no-action", base.part(2), check=False)
        results["scenarios"]["shrink"]["ntfsfix"] = {"status": fix.returncode, "output": (fix.stdout + fix.stderr)[-1500:]}
        check("shrink", "ntfsfix --no-action afterwards", None, f"status {fix.returncode}")
        # The volume is now marked for a check by Windows: refused.
        before_dirty = base.partition_sha(2)
        dirty = library("ntfs_look", base.part(2))
        results["scenarios"]["dirty"] = dirty
        check("dirty", "a volume marked for a check is refused",
              dirty.get("n_state") == "dirty" and dirty.get("n_code") == "TUR-INSTALL-SHRINK-004", str(dirty))
        check("dirty", "and left unchanged", base.partition_sha(2) == before_dirty)
    finally:
        base.detach()

    # --- refusals on crafted volumes ---------------------------------------------------
    for name in ("bitlocker", "hibernated", "too-full"):
        image = Image(work / f"{name}.raw", ntfs_mib=1024 if name == "too-full" else 2048, total_mib=4096)
        try:
            if name == "hibernated":
                header = work / "hiberfil.sys"
                header.write_bytes(b"HIBR" + os.urandom(1 * MIB - 4))
                run("ntfscp", image.part(2), str(header), "hiberfil.sys")
            elif name == "too-full":
                where = work / "mnt"
                mount(image.part(2), where)
                try:
                    with open(where / "big.bin", "wb") as stream:
                        stream.write(os.urandom(900 * MIB))
                finally:
                    umount(where)
            elif name == "bitlocker":
                with open(image.part(2), "r+b") as stream:
                    stream.write(b"\xebX\x90-FVE-FS-")
            before = image.partition_sha(2)
            look = library("ntfs_look", image.part(2))
            results["scenarios"][name] = look
            want = {"bitlocker": ("bitlocker", "TUR-INSTALL-SHRINK-001"),
                    "hibernated": ("hibernated", "TUR-INSTALL-SHRINK-003"),
                    "too-full": ("too-full", "TUR-INSTALL-SHRINK-005")}[name]
            check(name, f"refused with {want[1]}", (look.get("n_state"), look.get("n_code")) == want, str(look))
            check(name, "and left unchanged", image.partition_sha(2) == before)
        finally:
            image.detach()

    # --- interruptions, measured ------------------------------------------------------
    results["scenarios"]["interrupted"] = []
    for delay in (0.5, 1, 2, 4):
        copy = work / "far-cut.raw"
        run("cp", "--sparse=always", str(work / "far-pristine.raw"), str(copy))
        image = Image.__new__(Image)
        image.path, image.loop = copy, ""
        image.attach()
        try:
            look = library("ntfs_look", image.part(2))
            new = int(look.get("n_min") or 0) + 512 * MIB
            new -= new % MIB
            process = subprocess.Popen(["ntfsresize", "--force", "--no-progress-bar", "--size", str(new), image.part(2)],
                                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            process.stdin.write(b"y\n")
            process.stdin.flush()
            time.sleep(delay)
            finished = process.poll() is not None
            if not finished:
                process.send_signal(signal.SIGKILL)
            output = process.communicate()[0].decode(errors="replace")
            info = run("ntfsresize", "--info", "--force", "--no-progress-bar", image.part(2), check=False)
            fix = run("ntfsfix", "--no-action", image.part(2), check=False)
            now = file_hashes(image, list(hashes))
            item = {"killed_after_seconds": delay, "had_finished": finished,
                    "files_intact": sum(1 for k in hashes if now.get(k) == hashes[k]), "files": len(hashes),
                    "info_status": info.returncode, "info_tail": info.stdout[-400:],
                    "ntfsfix_status": fix.returncode, "ntfsfix_tail": (fix.stdout + fix.stderr)[-400:],
                    "ntfsresize_tail": output[-400:]}
            results["scenarios"]["interrupted"].append(item)
            check("interrupted", f"killed after {delay} s", None,
                  f"finished={finished}; files intact {item['files_intact']}/{item['files']}; "
                  f"ntfsresize --info status {info.returncode}; ntfsfix -n status {fix.returncode}")
        finally:
            image.detach()
            copy.unlink(missing_ok=True)

    failed = [row for row in checks if row[2] is False]
    results["checks"] = checks
    (out / "ntfs-shrink.json").write_text(json.dumps(results, indent=2) + "\n")
    lines = ["## Shrinking NTFS on crafted images (issue 64, step 2)", "", "| Scenario | Check | Result | Detail |",
             "|---|---|---|---|"]
    for scenario, what, ok, detail in checks:
        lines.append(f"| {scenario} | {what} | {'pass' if ok else ('**FAIL**' if ok is False else 'recorded')} | "
                     f"{detail.replace('|', '/')[:300]} |")
    lines += ["", "No Windows was involved: every volume was made by mkfs.ntfs (ntfs-3g). Real-Windows safety: NOT RUN."]
    (out / "summary.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
