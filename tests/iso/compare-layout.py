#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Compare a disk the installer medium installed with a disk the boot test
installed, as tests/boot/inspect-disk.py measured both from the host.

    tests/iso/compare-layout.py REFERENCE_OUT INSTALLED_OUT RESULT.json

REFERENCE_OUT/host and INSTALLED_OUT/host each hold disk-layout.json,
esp-files.txt, esp-configs.txt and boot-grub-config.txt. The reference is what
`tests/boot/build-disk.sh` produces today: the layout ADR-0014's boot counting
was proven on.

Compared: the partition table (number, name, type, start, size, filesystem and
label of every partition), the files on the EFI system partition with their
sizes, GRUB's configuration on both partitions, the fragments bootupd wrote
and their order, the size of GRUB's environment block, the number of loader
entries, and the kernel arguments of the loader entry as a set.

Not compared, because they differ between any two installations: filesystem
and partition UUIDs, the deployment checksum in `ostree=`, bytes used.

It writes the result and always exits 0. tests/iso/report.py decides.
"""

from __future__ import annotations

import json
import pathlib
import re
import sys

UUID = re.compile(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
FAT_ID = re.compile(r"\b[0-9A-F]{4}-[0-9A-F]{4}\b")
OSTREE = re.compile(r"ostree=/ostree/boot\.\d/[^/\s]+/[0-9a-f]{64}/\d+")
BOOTCSUM = re.compile(r"\b[0-9a-f]{64}\b")
PARTITION_KEYS = ("number", "name", "role", "type_guid", "start_bytes", "size_bytes", "filesystem", "filesystem_label")


def load(path: pathlib.Path):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return None


def text(path: pathlib.Path) -> str | None:
    try:
        return path.read_text(errors="replace")
    except OSError:
        return None


def normalise(value: str) -> str:
    value = OSTREE.sub("ostree=<deployment>", value)
    value = UUID.sub("<uuid>", value)
    value = FAT_ID.sub("<fat-id>", value)
    return BOOTCSUM.sub("<checksum>", value)


def arguments(options: list[str]) -> list[list[str]]:
    return [sorted(normalise(option).split()) for option in options]


def main() -> int:
    if len(sys.argv) != 4:
        print(__doc__, file=sys.stderr)
        return 2
    reference, installed, target = (pathlib.Path(arg) for arg in sys.argv[1:])
    ref, new = load(reference / "host" / "disk-layout.json"), load(installed / "host" / "disk-layout.json")
    compared, differences = [], []

    def compare(name: str, a, b) -> None:
        compared.append(name)
        if a != b:
            differences.append({"what": name, "reference": a, "installed_from_medium": b})

    if ref is None or new is None:
        differences.append({"what": "disk-layout.json", "reference": ref is not None, "installed_from_medium": new is not None})
    else:
        for key in ("label", "sector_size", "disk_bytes", "separate_boot_partition", "boot_entries", "grub_cfg_fragments",
                    "grub_cfg_counting_fragment_before_menu", "grubenv_bytes", "grub_config_mentions_boot_counter",
                    "esp_files", "inspection_errors"):
            compare(key, ref.get(key), new.get(key))
        compare(
            "partitions",
            [{key: part.get(key) for key in PARTITION_KEYS} for part in ref.get("partitions", [])],
            [{key: part.get(key) for key in PARTITION_KEYS} for part in new.get("partitions", [])],
        )
        compare(
            "EFI binaries carrying a signature",
            {name: info.get("signed") for name, info in (ref.get("esp_efi_binaries") or {}).items()},
            {name: info.get("signed") for name, info in (new.get("esp_efi_binaries") or {}).items()},
        )
        compare(
            "kernel arguments of the loader entry (as a set)",
            arguments(ref.get("boot_entry_options") or []),
            arguments(new.get("boot_entry_options") or []),
        )
    for name in ("esp-files.txt", "esp-configs.txt", "boot-grub-config.txt"):
        a, b = text(reference / "host" / name), text(installed / "host" / name)
        compare(name, normalise(a) if a is not None else None, normalise(b) if b is not None else None)

    order = {
        "reference": (ref or {}).get("boot_entry_options"),
        "installed_from_medium": (new or {}).get("boot_entry_options"),
    }
    result = {
        "identical": not differences,
        "compared": compared,
        "differences": differences,
        "kernel_arguments_in_order": order,
        "not_compared": ["filesystem and partition UUIDs", "the deployment checksum in ostree=", "bytes used"],
    }
    target.write_text(json.dumps(result, indent=2) + "\n")
    print(f"layout identical to the reference: {result['identical']}; compared: {', '.join(compared)}")
    for item in differences:
        print(f"DIFFERENT: {item['what']}\n  reference: {str(item['reference'])[:1500]}\n  medium:    {str(item['installed_from_medium'])[:1500]}")
    print(f"kernel arguments, in order: {json.dumps(order, indent=1)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
