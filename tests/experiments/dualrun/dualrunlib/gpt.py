# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Read a GUID partition table, read-only, and say which partitions are Windows'."""

from __future__ import annotations

import dataclasses
import struct
import uuid
import zlib

from .blockio import Dev

# Partition type GUIDs. Only the first group is ever given to the guest.
ESP = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"
MSR = "e3c9e316-0b5c-4db8-817d-f92df00215ae"
BASIC_DATA = "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7"
WINDOWS_RECOVERY = "de94bba4-06d1-4d40-a16a-bfd50179d6ac"
WINDOWS_TYPES = {ESP: "esp", MSR: "msr", BASIC_DATA: "windows-data", WINDOWS_RECOVERY: "windows-recovery"}

TYPE_NAMES = {
    ESP: "EFI system partition",
    MSR: "Microsoft reserved",
    BASIC_DATA: "Windows data (basic data)",
    WINDOWS_RECOVERY: "Windows recovery environment",
    "0fc63daf-8483-4772-8e79-3d69d8477de4": "Linux filesystem",
    "4f68bce3-e8cd-4db1-96e7-fbcaf984b709": "Linux root (x86-64)",
    "bc13c2ff-59e6-4262-a352-b275fd6f7172": "Linux extended boot loader",
    "0657fd6d-a4ab-43c4-84e5-0933c84b4f4f": "Linux swap",
    "e6d6d379-f507-44c2-a23c-238f2a3df928": "Linux LVM",
    "ca7d7ccb-63ed-4c53-861c-1742536059cc": "LUKS",
    "933ac7e1-2eb4-4f13-b844-0e14e2aef915": "Linux home",
    "21686148-6449-6e6f-744e-656564454649": "BIOS boot",
}


class GptError(Exception):
    """The disk has no usable GUID partition table."""


@dataclasses.dataclass
class Partition:
    index: int          # 1-based position in the table (the partition number)
    type_guid: str
    part_guid: str
    first_lba: int
    last_lba: int
    attributes: int
    name: str

    @property
    def sectors(self) -> int:
        return self.last_lba - self.first_lba + 1

    @property
    def role(self) -> str:
        return WINDOWS_TYPES.get(self.type_guid, "other")

    @property
    def type_name(self) -> str:
        return TYPE_NAMES.get(self.type_guid, "unknown type")


@dataclasses.dataclass
class Gpt:
    sector: int
    total_sectors: int
    disk_guid: str
    first_usable: int
    last_usable: int
    entries_lba: int
    entries_sectors: int
    alt_lba: int
    partitions: list[Partition]

    @property
    def head_sectors(self) -> int:
        """Sectors from the start of the disk to the first partition."""
        return min(p.first_lba for p in self.partitions)

    @property
    def tail_start(self) -> int:
        """First sector of the backup table at the end of the disk."""
        return self.alt_lba - self.entries_sectors


def _guid(raw: bytes) -> str:
    return str(uuid.UUID(bytes_le=bytes(raw)))


def read_gpt(disk: Dev, sector: int | None = None) -> Gpt:
    sector = sector or disk.sector
    if disk.size < 68 * sector:
        raise GptError("the disk is too small to hold a partition table")
    header = disk.pread(sector, sector)
    if header[:8] != b"EFI PART":
        raise GptError("no GUID partition table (the disk is partitioned the old BIOS way, or is empty)")
    size, = struct.unpack_from("<I", header, 12)
    crc, = struct.unpack_from("<I", header, 16)
    if not 92 <= size <= sector:
        raise GptError("the partition table header has an impossible size")
    check = bytearray(header[:size])
    check[16:20] = b"\0\0\0\0"
    if zlib.crc32(bytes(check)) & 0xFFFFFFFF != crc:
        raise GptError("the partition table header fails its checksum")
    my_lba, alt_lba, first_usable, last_usable = struct.unpack_from("<QQQQ", header, 24)
    disk_guid = _guid(header[56:72])
    entries_lba, count, entry_size, entries_crc = struct.unpack_from("<QIII", header, 72)
    if my_lba != 1 or entry_size < 128 or count > 1024:
        raise GptError("the partition table header is not in a form this program reads")
    raw = disk.pread(entries_lba * sector, count * entry_size)
    if zlib.crc32(raw) & 0xFFFFFFFF != entries_crc:
        raise GptError("the partition entries fail their checksum")
    total = disk.size // sector
    partitions = []
    for i in range(count):
        entry = raw[i * entry_size:(i + 1) * entry_size]
        if entry[:16] == b"\0" * 16:
            continue
        first, last, attributes = struct.unpack_from("<QQQ", entry, 32)
        name = entry[56:128].decode("utf-16-le", "replace").split("\0", 1)[0]
        if first > last or last >= total:
            raise GptError(f"partition {i + 1} lies outside the disk")
        partitions.append(Partition(i + 1, _guid(entry[:16]), _guid(entry[16:32]), first, last, attributes, name))
    if not partitions:
        raise GptError("the partition table is empty")
    ordered = sorted(partitions, key=lambda p: p.first_lba)
    for a, b in zip(ordered, ordered[1:]):
        if b.first_lba <= a.last_lba:
            raise GptError(f"partitions {a.index} and {b.index} overlap")
    entries_sectors = -(-count * entry_size // sector)
    gpt = Gpt(sector, total, disk_guid, first_usable, last_usable, entries_lba, entries_sectors, alt_lba, partitions)
    if alt_lba != total - 1:
        raise GptError("the backup partition table is not at the end of the disk "
                       "(the disk was resized or copied; Windows itself would want to repair this)")
    if gpt.head_sectors < entries_lba + entries_sectors or ordered[-1].last_lba >= gpt.tail_start:
        raise GptError("a partition overlaps the partition table")
    return gpt
