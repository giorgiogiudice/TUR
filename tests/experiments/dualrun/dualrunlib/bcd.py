# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Read Windows' boot configuration (the BCD store) from bytes, read-only.

The store is a registry hive on the EFI system partition, which is never
encrypted, so it can be read even when the Windows volume is locked by
BitLocker. What the check-list wants from it: the boot manager's
"attempt resume" element, which says that the next start of Windows will
resume from a hibernation image.

Only the parts of the hive format needed to walk keys and read small values
are implemented. Anything unexpected raises BcdError, and the caller treats
that as "unknown".
"""

from __future__ import annotations

import struct

BOOTMGR = "{9dea862c-5cdd-4e70-acc1-f32b344d4795}"
ATTEMPT_RESUME = "26000005"
RESUME_OBJECT = "23000006"
DEFAULT_OBJECT = "23000003"
ELEMENT_NAMES = {
    "11000001": "device", "12000002": "path", "12000004": "description", "12000005": "locale",
    "14000006": "inherit", "23000003": "default", "23000006": "resumeobject", "24000001": "displayorder",
    "24000002": "bootsequence", "24000010": "toolsdisplayorder", "25000004": "timeout",
    "26000005": "resume (attempt resume)", "21000001": "osdevice", "22000002": "systemroot",
    "23000003/osloader": "resumeobject", "25000020": "nx", "250000f0": "bootmenupolicy",
    "26000022": "winpe", "16000009": "recoveryenabled", "14000008": "recoverysequence",
    "21000005": "filedevice", "22000006": "filepath", "250000f3": "bootstatuspolicy",
    "26000010": "detecthal", "16000060": "isolatedcontext", "16000048": "nointegritychecks",
}


class BcdError(Exception):
    """The store is not a hive this program can read."""


class Hive:
    def __init__(self, data: bytes):
        if data[:4] != b"regf" or len(data) < 0x1020:
            raise BcdError("not a registry hive")
        self.data = data
        self.root = struct.unpack_from("<I", data, 0x24)[0]
        self.sequence = struct.unpack_from("<II", data, 4)

    def _cell(self, offset: int) -> bytes:
        position = 0x1000 + offset
        if offset == 0xFFFFFFFF or position + 4 > len(self.data):
            raise BcdError("a cell lies outside the hive")
        size = abs(struct.unpack_from("<i", self.data, position)[0])
        if size < 8 or position + size > len(self.data):
            raise BcdError("a cell has an impossible size")
        return self.data[position + 4:position + size]

    def _key(self, offset: int) -> dict:
        cell = self._cell(offset)
        if cell[:2] != b"nk":
            raise BcdError("expected a key cell")
        flags, = struct.unpack_from("<H", cell, 2)
        subkeys, = struct.unpack_from("<I", cell, 0x14)
        subkey_list, = struct.unpack_from("<I", cell, 0x1C)
        values, value_list = struct.unpack_from("<II", cell, 0x24)
        name_length, = struct.unpack_from("<H", cell, 0x48)
        raw = cell[0x4C:0x4C + name_length]
        name = raw.decode("latin-1") if flags & 0x20 else raw.decode("utf-16-le", "replace")
        return {"name": name, "subkeys": subkeys, "subkey_list": subkey_list,
                "values": values, "value_list": value_list}

    def subkeys(self, offset: int) -> dict[str, int]:
        key = self._key(offset)
        out: dict[str, int] = {}
        if key["subkeys"] == 0:
            return out

        def walk(list_offset: int, depth: int = 0) -> None:
            cell = self._cell(list_offset)
            kind, count = cell[:2], struct.unpack_from("<H", cell, 2)[0]
            if kind in (b"lf", b"lh"):
                for i in range(count):
                    child, = struct.unpack_from("<I", cell, 4 + 8 * i)
                    out[self._key(child)["name"].lower()] = child
            elif kind == b"li":
                for i in range(count):
                    child, = struct.unpack_from("<I", cell, 4 + 4 * i)
                    out[self._key(child)["name"].lower()] = child
            elif kind == b"ri" and depth < 4:
                for i in range(count):
                    walk(struct.unpack_from("<I", cell, 4 + 4 * i)[0], depth + 1)
            else:
                raise BcdError("unknown subkey list")

        walk(key["subkey_list"])
        return out

    def values(self, offset: int) -> dict[str, tuple[int, bytes]]:
        key = self._key(offset)
        out: dict[str, tuple[int, bytes]] = {}
        if key["values"] == 0:
            return out
        table = self._cell(key["value_list"])
        for i in range(key["values"]):
            cell = self._cell(struct.unpack_from("<I", table, 4 * i)[0])
            if cell[:2] != b"vk":
                raise BcdError("expected a value cell")
            name_length, size, data_offset, kind, flags = struct.unpack_from("<HIIIH", cell, 2)
            raw = cell[0x14:0x14 + name_length]
            name = raw.decode("latin-1") if flags & 1 else raw.decode("utf-16-le", "replace")
            if size & 0x80000000:
                data = struct.pack("<I", data_offset)[:size & 0x7FFFFFFF]
            elif size > 16344:
                data = b""           # stored in pieces; nothing this program needs is that large
            else:
                data = self._cell(data_offset)[:size]
            out[name.lower()] = (kind, data)
        return out

    def open(self, path: str) -> int | None:
        offset = self.root
        for part in [p for p in path.split("\\") if p]:
            offset = self.subkeys(offset).get(part.lower())
            if offset is None:
                return None
        return offset


def read_store(data: bytes) -> dict:
    """{'objects': {guid: {element: hex}}, 'resume_pending': bool | None}."""
    hive = Hive(data)
    objects_key = hive.open("Objects")
    if objects_key is None:
        raise BcdError("the store has no Objects key")
    objects: dict[str, dict[str, str]] = {}
    for guid, offset in hive.subkeys(objects_key).items():
        elements: dict[str, str] = {}
        children = hive.subkeys(offset)
        if "elements" in children:
            for element, element_offset in hive.subkeys(children["elements"]).items():
                value = hive.values(element_offset).get("element")
                if value is not None:
                    elements[element] = value[1].hex()
        description = hive.values(children["description"]).get("type") if "description" in children else None
        if description is not None and len(description[1]) >= 4:
            elements["_type"] = f"{struct.unpack_from('<I', description[1])[0]:08x}"
        objects[guid] = elements
    manager = objects.get(BOOTMGR)
    resume = None
    if manager is not None:
        flag = manager.get(ATTEMPT_RESUME)
        resume = bool(flag) and any(b != "0" for b in flag)
    return {"objects": objects, "resume_pending": resume, "hive_sequence": list(hive.sequence)}


def summary(store: dict) -> dict:
    """What is worth logging: no device blobs, only small elements of the boot manager."""
    manager = store["objects"].get(BOOTMGR, {})
    small = {}
    for element, value in sorted(manager.items()):
        if element.startswith("_") or len(value) > 160:
            continue
        small[f"{element} {ELEMENT_NAMES.get(element, '')}".strip()] = value
    return {"resume_pending": store["resume_pending"], "objects": len(store["objects"]),
            "hive_sequence": store["hive_sequence"], "bootmgr": small}
