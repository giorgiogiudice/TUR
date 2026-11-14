#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""What is on a disk image that holds Windows, and whether it is still there.

    tests/iso/windows-disk.py record DISK OUT.json
    tests/iso/windows-disk.py compare BEFORE.json DISK OUT.json [--shrunk N]

With --shrunk N (shrinking Windows from the installer, issue 64 step 2),
partition N may be smaller, with the same start, type, UUID, name and
attributes, and its bytes may differ; instead, every file in its NTFS root
must have its SHA-256 (read with ntfscat through a read-only loop device:
run as root, also for `record`).

The proof of "install TUR beside Windows on the same disk" (ADR-0029):
everything Windows had must be byte-for-byte where it was.

`record` reads, from the host, without starting anything:

- the GPT, with `sfdisk --json` (and `sfdisk --verify`);
- the SHA-256 of the first sector (the protective MBR) and of every partition,
  read straight from the image by its byte range;
- every file on the EFI system partition, copied out with mtools (`mcopy`,
  read-only) and hashed, and `fsck.fat -n` of a copy of the partition;
- the free stretches (`sfdisk --list-free`).

`compare` records again and checks, against BEFORE:

- every partition entry that was there is still there, unchanged (number,
  start, size, type, UUID, name, attributes);
- every partition that was there, except the EFI system partition, has the
  same SHA-256: none of its bytes changed;
- every file that was on the EFI system partition is there with the same
  SHA-256; files added are listed (they must be TUR's own);
- a partition added lies wholly inside a stretch that was free before;
- whether the first sector (protective MBR) is unchanged, reported only.

It writes OUT.json and exits 1 if anything of Windows' changed. Run in CI only
(ADR-0002), on disposable disk images.
"""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import subprocess
import sys
import tempfile

ESP = "C12A7328-F81F-11D2-BA4B-00A0C93EC93B"
SECTOR = 512


def run(*command: str, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(command, capture_output=True, text=True, check=check)


def sha256_range(path: pathlib.Path, start: int, size: int) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        stream.seek(start)
        left = size
        while left > 0:
            chunk = stream.read(min(left, 1 << 22))
            if not chunk:
                break
            digest.update(chunk)
            left -= len(chunk)
    return digest.hexdigest()


def esp_files(disk: pathlib.Path, offset: int, size: int) -> dict:
    """Every file on the FAT partition at OFFSET, hashed; and fsck.fat -n."""
    facts: dict = {"files": {}, "fsck": None}
    env = dict(os.environ, MTOOLS_SKIP_CHECK="1")
    with tempfile.TemporaryDirectory() as tmp:
        tree = pathlib.Path(tmp) / "tree"
        tree.mkdir()
        copy = subprocess.run(["mcopy", "-s", "-n", "-p", "-i", f"{disk}@@{offset}", "::/*", str(tree)],
                              capture_output=True, text=True, env=env)
        facts["mcopy_status"] = copy.returncode
        if copy.returncode != 0:
            facts["mcopy_error"] = copy.stderr.strip()[-2000:]
        for path in sorted(tree.rglob("*")):
            if path.is_file():
                facts["files"][str(path.relative_to(tree))] = hashlib.sha256(path.read_bytes()).hexdigest()
        image = pathlib.Path(tmp) / "esp.img"
        with open(disk, "rb") as source, open(image, "wb") as target:
            source.seek(offset)
            left = size
            while left > 0:
                chunk = source.read(min(left, 1 << 22))
                if not chunk:
                    break
                target.write(chunk)
                left -= len(chunk)
        check = subprocess.run(["fsck.fat", "-n", "-v", str(image)], capture_output=True, text=True)
        facts["fsck"] = {"status": check.returncode, "output": (check.stdout + check.stderr).strip()[-3000:]}
        info = subprocess.run(["minfo", "-i", f"{disk}@@{offset}", "::"], capture_output=True, text=True, env=env)
        facts["minfo"] = info.stdout.strip()[-1500:]
    return facts


def ntfs_files(disk: pathlib.Path, offset: int, size: int) -> dict:
    """The files in an NTFS volume's root and their SHA-256 (root only)."""
    loop = run("losetup", "--find", "--show", "--read-only", "--offset", str(offset), "--sizelimit", str(size),
               str(disk)).stdout.strip()
    found: dict = {}
    try:
        names = run("ntfsls", "--force", loop, check=False).stdout.split("\n")
        for name in (n.strip() for n in names):
            if not name or name.startswith("$") or name in (".", ".."):
                continue
            data = subprocess.run(["ntfscat", "--force", loop, name], capture_output=True)
            found[name] = hashlib.sha256(data.stdout).hexdigest() if data.returncode == 0 else None
    finally:
        run("losetup", "--detach", loop, check=False)
    return found


def record(disk: pathlib.Path) -> dict:
    table = json.loads(run("sfdisk", "--json", str(disk)).stdout)["partitiontable"]
    verify = run("sfdisk", "--verify", str(disk), check=False)
    free = run("sfdisk", "--list-free", "--bytes", str(disk), check=False).stdout
    facts: dict = {
        "disk": str(disk),
        "disk_bytes": disk.stat().st_size,
        "label_id": table.get("id"),
        "first_lba": table.get("firstlba"),
        "last_lba": table.get("lastlba"),
        "verify": {"status": verify.returncode, "output": (verify.stdout + verify.stderr).strip()},
        "free": free.strip(),
        "free_regions": [],
        "mbr_sha256": sha256_range(disk, 0, SECTOR),
        "partitions": [],
    }
    for line in free.splitlines():
        fields = line.split()
        if len(fields) >= 3 and all(field.isdigit() for field in fields[:3]):
            facts["free_regions"].append({"start_sector": int(fields[0]), "end_sector": int(fields[1])})
    for number, part in enumerate(table.get("partitions", []), start=1):
        node = part["node"]
        index = int("".join(ch for ch in node[len(str(disk)):] if ch.isdigit()) or number)
        start, sectors = int(part["start"]), int(part["size"])
        entry = {
            "number": index,
            "start": start,
            "size": sectors,
            "type": part.get("type", "").upper(),
            "uuid": part.get("uuid", "").upper(),
            "name": part.get("name", ""),
            "attrs": part.get("attrs", ""),
        }
        entry["sha256"] = sha256_range(disk, start * SECTOR, sectors * SECTOR)
        with open(disk, "rb") as stream:
            stream.seek(start * SECTOR + 3)
            if stream.read(8) == b"NTFS    " and os.geteuid() == 0:
                entry["ntfs_files"] = ntfs_files(disk, start * SECTOR, sectors * SECTOR)
        if entry["type"] == ESP:
            entry["esp"] = esp_files(disk, start * SECTOR, sectors * SECTOR)
        facts["partitions"].append(entry)
    return facts


def key(entry: dict) -> tuple:
    return tuple(entry[name] for name in ("number", "start", "size", "type", "uuid", "name", "attrs"))


def compare(before: dict, after: dict, shrunk: int | None = None) -> dict:
    checks: dict = {"problems": [], "shrunk_partition": shrunk}
    now = {entry["number"]: entry for entry in after["partitions"]}
    kept = []
    for old in before["partitions"]:
        new = now.get(old["number"])
        same_entry = new is not None and key(new) == key(old)
        item = {"number": old["number"], "name": old["name"], "type": old["type"], "entry_unchanged": same_entry}
        if old["number"] == shrunk:
            same_but_size = new is not None and all(new[k] == old[k] for k in ("number", "start", "type", "uuid", "name", "attrs"))
            files_before, files_after = old.get("ntfs_files") or {}, (new or {}).get("ntfs_files") or {}
            same_files = sorted(name for name, digest in files_before.items() if files_after.get(name) == digest)
            item.update({"shrunk": True, "same_entry_but_size": same_but_size,
                         "sectors_before": old["size"], "sectors_after": (new or {}).get("size"),
                         "files_before": len(files_before), "files_unchanged": len(same_files),
                         "files_changed_or_missing": sorted(set(files_before) - set(same_files))})
            if not same_but_size or (new or {}).get("size", old["size"]) >= old["size"]:
                checks["problems"].append(f"partition {old['number']}: not shrunk in place")
            if not files_before or len(same_files) != len(files_before):
                checks["problems"].append(f"partition {old['number']}: files changed or missing after the shrink")
            kept.append(item)
            continue
        if not same_entry:
            checks["problems"].append(f"partition {old['number']}: its table entry changed or is gone")
        if old.get("esp") is not None:
            files_before, files_after = old["esp"]["files"], (new or {}).get("esp", {}).get("files", {})
            changed = sorted(path for path, digest in files_before.items() if files_after.get(path) != digest)
            added = sorted(path for path in files_after if path not in files_before)
            item.update({
                "esp_files_before": len(files_before),
                "esp_files_unchanged": len(files_before) - len(changed),
                "esp_files_changed_or_missing": changed,
                "esp_files_added": added,
                "esp_fsck_status_after": (new or {}).get("esp", {}).get("fsck", {}).get("status"),
                "esp_bytes_unchanged": new is not None and new["sha256"] == old["sha256"],
            })
            if changed:
                checks["problems"].append(f"EFI system partition: files changed or missing: {changed}")
            if not files_before:
                checks["problems"].append("EFI system partition: no file could be read before")
        else:
            same_bytes = new is not None and new["sha256"] == old["sha256"]
            item["bytes_unchanged"] = same_bytes
            if not same_bytes:
                checks["problems"].append(f"partition {old['number']} ({old['name']}): its bytes changed")
        kept.append(item)
    checks["windows_partitions"] = kept
    added = [entry for number, entry in sorted(now.items()) if number not in {old["number"] for old in before["partitions"]}]
    placed = []
    for entry in added:
        last = entry["start"] + entry["size"] - 1
        regions = list(before["free_regions"])
        if shrunk is not None:
            # What the shrink freed counts as free space.
            old = next(p for p in before["partitions"] if p["number"] == shrunk)
            new = next((p for p in after["partitions"] if p["number"] == shrunk), old)
            regions.append({"start_sector": new["start"] + new["size"], "end_sector": old["start"] + old["size"] - 1})
            regions = merge(regions)
        inside = any(region["start_sector"] <= entry["start"] and last <= region["end_sector"] for region in regions)
        placed.append({"number": entry["number"], "name": entry["name"], "type": entry["type"],
                       "start": entry["start"], "size": entry["size"], "inside_free_space_before": inside})
        if not inside:
            checks["problems"].append(f"partition {entry['number']} was added outside the free space")
    checks["added_partitions"] = placed
    # Reported, not judged: libfdisk writes the protective MBR again with
    # every GPT it writes. It holds no Windows data; on a disk Windows
    # partitioned its bytes may differ from what libfdisk writes.
    checks["mbr_unchanged"] = before["mbr_sha256"] == after["mbr_sha256"]
    checks["gpt_verify_after"] = after["verify"]
    checks["label_id_unchanged"] = before["label_id"] == after["label_id"]
    checks["windows_intact"] = not checks["problems"]
    return checks


def merge(regions: list[dict]) -> list[dict]:
    out: list[dict] = []
    for region in sorted(regions, key=lambda r: r["start_sector"]):
        if out and region["start_sector"] <= out[-1]["end_sector"] + 1:
            out[-1]["end_sector"] = max(out[-1]["end_sector"], region["end_sector"])
        else:
            out.append(dict(region))
    return out


def main() -> int:
    if len(sys.argv) >= 4 and sys.argv[1] == "record":
        facts = record(pathlib.Path(sys.argv[2]))
        pathlib.Path(sys.argv[3]).write_text(json.dumps(facts, indent=2) + "\n")
        esp = next((p for p in facts["partitions"] if p.get("esp")), None)
        print(f"recorded {len(facts['partitions'])} partitions; ESP files: "
              f"{len(esp['esp']['files']) if esp else 'none'}; free: {facts['free_regions']}")
        return 0
    if len(sys.argv) >= 5 and sys.argv[1] == "compare":
        before = json.loads(pathlib.Path(sys.argv[2]).read_text())
        after = record(pathlib.Path(sys.argv[3]))
        shrunk = int(sys.argv[sys.argv.index("--shrunk") + 1]) if "--shrunk" in sys.argv else None
        result = compare(before, after, shrunk)
        result["after"] = {k: v for k, v in after.items() if k != "partitions"}
        result["after_partitions"] = [{k: v for k, v in p.items() if k != "esp"} for p in after["partitions"]]
        pathlib.Path(sys.argv[4]).write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps({k: v for k, v in result.items() if k not in ("after", "after_partitions")}, indent=2))
        return 0 if result["windows_intact"] else 1
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
