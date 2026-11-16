# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""A small reader of Windows registry files ("regf" hives: NTUSER.DAT,
SOFTWARE, SYSTEM) that uses nothing of Windows. The experiment uses it to
show that what a running Windows reports is what the file on the disk
holds (docs/reports/exp-inwin-1.md, section 7). TUR itself would use the
hivex library; this is the same format read independently, in about a
hundred lines, to test the claim rather than the library.

Reads only. Handles the key and value cells, the four kinds of subkey
list and values larger than one cell. Does not replay transaction logs:
a hive saved by `reg save`, or one Windows shut down cleanly, has none
pending.
"""

import struct


class Hive:
    def __init__(self, data):
        if data[:4] != b"regf":
            raise ValueError("not a registry file")
        self.data = data
        self.root = struct.unpack_from("<I", data, 0x24)[0]

    def _cell(self, offset):
        """The bytes of a cell, without its size field."""
        start = 0x1000 + offset
        size = abs(struct.unpack_from("<i", self.data, start)[0])
        return self.data[start + 4:start + size]

    def _name(self, raw, compressed):
        return raw.decode("latin-1") if compressed else raw.decode("utf-16-le", "replace")

    def _subkeys(self, offset):
        cell = self._cell(offset)
        kind = cell[:2]
        count = struct.unpack_from("<H", cell, 2)[0]
        if kind in (b"lf", b"lh"):
            return [struct.unpack_from("<I", cell, 4 + 8 * i)[0] for i in range(count)]
        if kind == b"li":
            return [struct.unpack_from("<I", cell, 4 + 4 * i)[0] for i in range(count)]
        if kind == b"ri":
            found = []
            for i in range(count):
                found += self._subkeys(struct.unpack_from("<I", cell, 4 + 4 * i)[0])
            return found
        raise ValueError(f"unknown subkey list {kind!r}")

    def _key_name(self, offset):
        cell = self._cell(offset)
        if cell[:2] != b"nk":
            raise ValueError("not a key")
        flags = struct.unpack_from("<H", cell, 2)[0]
        length = struct.unpack_from("<H", cell, 0x48)[0]
        return self._name(cell[0x4C:0x4C + length], flags & 0x20)

    def key(self, path):
        """The offset of the key at `path` (backslashes), or raises KeyError."""
        offset = self.root
        for part in [p for p in path.split("\\") if p]:
            cell = self._cell(offset)
            count, listing = struct.unpack_from("<I", cell, 0x14)[0], struct.unpack_from("<I", cell, 0x1C)[0]
            found = None
            if count and listing != 0xFFFFFFFF:
                for child in self._subkeys(listing):
                    if self._key_name(child).lower() == part.lower():
                        found = child
                        break
            if found is None:
                raise KeyError(path)
            offset = found
        return offset

    def subkeys(self, path):
        cell = self._cell(self.key(path))
        count, listing = struct.unpack_from("<I", cell, 0x14)[0], struct.unpack_from("<I", cell, 0x1C)[0]
        if not count or listing == 0xFFFFFFFF:
            return []
        return [self._key_name(child) for child in self._subkeys(listing)]

    def values(self, path):
        """Every value of a key: name -> (type, value)."""
        cell = self._cell(self.key(path))
        count, listing = struct.unpack_from("<I", cell, 0x24)[0], struct.unpack_from("<I", cell, 0x28)[0]
        found = {}
        if not count or listing == 0xFFFFFFFF:
            return found
        offsets = self._cell(listing)
        for i in range(count):
            vk = self._cell(struct.unpack_from("<I", offsets, 4 * i)[0])
            if vk[:2] != b"vk":
                continue
            name_length, size, where, kind, flags = struct.unpack_from("<HIIIH", vk, 2)
            name = self._name(vk[0x14:0x14 + name_length], flags & 1)
            if size & 0x80000000:
                raw = struct.pack("<I", where)[:size & 0x7FFFFFFF]
            else:
                raw = self._data(where, size)
            found[name] = (kind, self._decode(kind, raw))
        return found

    def _data(self, offset, size):
        cell = self._cell(offset)
        if size > 16344 and cell[:2] == b"db":
            segments = struct.unpack_from("<H", cell, 2)[0]
            listing = self._cell(struct.unpack_from("<I", cell, 4)[0])
            raw = b""
            for i in range(segments):
                raw += self._cell(struct.unpack_from("<I", listing, 4 * i)[0])[:16344]
            return raw[:size]
        return cell[:size]

    @staticmethod
    def _decode(kind, raw):
        if kind in (1, 2):
            return raw.decode("utf-16-le", "replace").split("\0", 1)[0]
        if kind == 4 and len(raw) >= 4:
            return struct.unpack("<I", raw[:4])[0]
        if kind == 11 and len(raw) >= 8:
            return struct.unpack("<Q", raw[:8])[0]
        if kind == 7:
            return [part for part in raw.decode("utf-16-le", "replace").split("\0") if part]
        return bytes(raw)

    def value(self, path, name):
        return self.values(path)[name][1]
