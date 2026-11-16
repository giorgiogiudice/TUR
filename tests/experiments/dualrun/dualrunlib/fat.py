# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Read files from a FAT16 or FAT32 volume (an EFI system partition), read-only.

Used to see whether the partition holds Windows' boot manager, to read the
boot configuration (`EFI/Microsoft/Boot/BCD`) and to keep a copy and a
checksum of the person's own boot files, without mounting anything.
"""

from __future__ import annotations

import struct

from .blockio import Dev


class FatError(Exception):
    """The volume could not be read as FAT16 or FAT32."""


class Fat:
    def __init__(self, dev: Dev):
        self.dev = dev
        boot = dev.pread(0, 512)
        if boot[510:512] != b"\x55\xaa":
            raise FatError("no boot sector signature")
        self.bps, spc, reserved, fats, root_entries, total16 = struct.unpack_from("<HBHBHH", boot, 11)
        fat16, = struct.unpack_from("<H", boot, 22)
        total32, fat32, = struct.unpack_from("<I", boot, 32)[0], struct.unpack_from("<I", boot, 36)[0]
        if self.bps not in (512, 1024, 2048, 4096) or spc == 0 or fats == 0:
            raise FatError("the boot sector's numbers are not plausible")
        fat_sectors = fat16 or fat32
        total = total16 or total32
        root_sectors = -(-root_entries * 32 // self.bps)
        self.cluster = spc * self.bps
        self.fat_offset = reserved * self.bps
        self.root_offset = (reserved + fats * fat_sectors) * self.bps
        self.data_offset = self.root_offset + root_sectors * self.bps
        clusters = (total - (reserved + fats * fat_sectors + root_sectors)) // spc
        # FAT32 is told by its boot sector (no 16-bit table size, no fixed root), not by
        # the cluster count: tools make small FAT32 volumes that the count rule would misread.
        self.bits = 32 if fat16 == 0 and root_entries == 0 else 16
        if self.bits == 16 and clusters < 4085:
            raise FatError("FAT12 volumes are not read")
        self.root_entries = root_entries
        self.root_cluster = struct.unpack_from("<I", boot, 44)[0] if self.bits == 32 else 0
        self.label = (boot[71:82] if self.bits == 32 else boot[43:54]).decode("latin-1").strip()
        self._fat = dev.pread(self.fat_offset, min(fat_sectors * self.bps, (clusters + 2) * self.bits // 8))

    def _next(self, cluster: int) -> int | None:
        if self.bits == 16:
            value, = struct.unpack_from("<H", self._fat, cluster * 2)
            return None if value >= 0xFFF8 or value < 2 else value
        value = struct.unpack_from("<I", self._fat, cluster * 4)[0] & 0x0FFFFFFF
        return None if value >= 0x0FFFFFF8 or value < 2 else value

    def _chain(self, cluster: int, limit: int | None = None) -> bytes:
        out, seen = bytearray(), 0
        while cluster is not None and cluster >= 2:
            out += self.dev.pread(self.data_offset + (cluster - 2) * self.cluster, self.cluster)
            seen += 1
            if seen > 1_000_000 or (limit is not None and len(out) >= limit):
                break
            cluster = self._next(cluster)
        return bytes(out)

    def _directory(self, cluster: int | None) -> list[dict]:
        if cluster is None:
            raw = (self._chain(self.root_cluster) if self.bits == 32
                   else self.dev.pread(self.root_offset, self.root_entries * 32))
        else:
            raw = self._chain(cluster)
        entries, long_parts = [], []
        for pos in range(0, len(raw) - 31, 32):
            entry = raw[pos:pos + 32]
            first, attributes = entry[0], entry[11]
            if first == 0:
                break
            if first == 0xE5:
                long_parts = []
                continue
            if attributes == 0x0F:
                chars = entry[1:11] + entry[14:26] + entry[28:32]
                long_parts.insert(0, chars.decode("utf-16-le", "replace"))
                continue
            if attributes & 0x08:      # the volume label
                long_parts = []
                continue
            base = entry[0:8].decode("latin-1").rstrip()
            extension = entry[8:11].decode("latin-1").rstrip()
            if entry[12] & 0x08:       # stored without a long name, shown in lower case
                base = base.lower()
            if entry[12] & 0x10:
                extension = extension.lower()
            short = base + ("." + extension if extension else "")
            long_name = "".join(long_parts).split("\0", 1)[0].rstrip("￿")
            long_parts = []
            high, low, size = struct.unpack_from("<H", entry, 20)[0], struct.unpack_from("<H", entry, 26)[0], \
                struct.unpack_from("<I", entry, 28)[0]
            if short in (".", ".."):
                continue
            entries.append({"name": long_name or short, "short": short, "directory": bool(attributes & 0x10),
                            "cluster": (high << 16 | low) if self.bits == 32 else low, "size": size})
        return entries

    def _find(self, path: str) -> dict | None:
        cluster, found = None, {"directory": True, "cluster": None, "size": 0, "name": ""}
        for part in [p for p in path.replace("\\", "/").split("/") if p]:
            if not found["directory"]:
                return None
            wanted = part.lower()
            for entry in self._directory(cluster):
                if entry["name"].lower() == wanted or entry["short"].lower() == wanted:
                    found = entry
                    cluster = entry["cluster"]
                    break
            else:
                return None
        return found

    def exists(self, path: str) -> bool:
        return self._find(path) is not None

    def read(self, path: str, limit: int = 64 * 1024 * 1024) -> bytes:
        entry = self._find(path)
        if entry is None or entry["directory"]:
            raise FatError(f"no such file: {path}")
        if entry["size"] > limit:
            raise FatError(f"{path} is larger than {limit} bytes")
        if entry["size"] == 0:
            return b""
        return self._chain(entry["cluster"], entry["size"])[:entry["size"]]

    def walk(self, path: str = "") -> list[tuple[str, int]]:
        """Every file under `path` as (path, size), sorted, case kept."""
        start = self._find(path) if path else {"directory": True, "cluster": None}
        if start is None or not start["directory"]:
            return []
        out, todo = [], [(path.strip("/"), start["cluster"])]
        while todo:
            prefix, cluster = todo.pop()
            for entry in self._directory(cluster):
                full = f"{prefix}/{entry['name']}" if prefix else entry["name"]
                if entry["directory"]:
                    if len(todo) < 10000:
                        todo.append((full, entry["cluster"]))
                else:
                    out.append((full, entry["size"]))
        return sorted(out)
