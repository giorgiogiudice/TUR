# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The guest's firmware variables: a boot entry that names Windows' boot manager.

Why TUR cannot leave this to the firmware. A guest started with fresh
firmware variables has no "Windows Boot Manager" entry, so OVMF falls back to
`EFI/BOOT/BOOTX64.EFI`. On a disk TUR shares with Windows that file can be
TUR's own shim (bootupd writes it; pull request 112 puts Windows' copy back,
and a later shim update overwrites it again), and the guest would then try
to start TUR inside the guest. So the launcher writes one entry itself,
before the guest starts: the EFI system partition by its partition GUID, and
`\\EFI\\Microsoft\\Boot\\bootmgfw.efi`, first in `BootOrder`.

The file format is edk2's variable store (MdeModulePkg/Include/Guid/
VariableFormat.h), the same one tests/iso/efivars.py reads. This module only
appends complete variables to the unused end of the store, and marks an
older variable of the same name deleted, as the firmware itself does.
"""

from __future__ import annotations

import struct
import uuid

GLOBAL = uuid.UUID("8be4df61-93ca-11d2-aa0d-00e098032b8c")
AUTH_STORE = uuid.UUID("aaf32c78-947b-439a-a180-2e144ec37792")
PLAIN_STORE = uuid.UUID("ddcf3616-3275-4164-98b6-fe85707ffe7d")
VAR_ADDED = 0x3F
VAR_DELETED = 0xFD
NV_BS_RT = 0x7
BOOTMGFW = "\\EFI\\Microsoft\\Boot\\bootmgfw.efi"


class VarsError(Exception):
    """The file is not a variable store this module can change."""


def _store(data: bytes) -> tuple[int, int, bool]:
    """(offset of the first variable, end of the store, authenticated?)."""
    for guid, auth in ((AUTH_STORE, True), (PLAIN_STORE, False)):
        at = data.find(guid.bytes_le)
        if at >= 0:
            size, = struct.unpack_from("<I", data, at + 16)
            if data[at + 20] != 0x5A or size < 28 or at + size > len(data):
                raise VarsError("the variable store header is not in the expected form")
            return at + 28, at + size, auth
    raise VarsError("no variable store found in the file")


def variables(data: bytes) -> list[dict]:
    offset, end, auth = _store(data)
    header = 60 if auth else 32
    found = []
    while offset + header <= end and data[offset:offset + 2] == b"\xaa\x55":
        state = data[offset + 2]
        attributes, = struct.unpack_from("<I", data, offset + 4)
        name_size, data_size = struct.unpack_from("<II", data, offset + (36 if auth else 8))
        guid = uuid.UUID(bytes_le=bytes(data[offset + header - 16:offset + header]))
        name_at = offset + header
        name = bytes(data[name_at:name_at + name_size]).decode("utf-16-le", "replace").rstrip("\0")
        value = bytes(data[name_at + name_size:name_at + name_size + data_size])
        following = (name_at + name_size + data_size + 3) & ~3
        found.append({"offset": offset, "next": following, "name": name, "guid": guid, "state": state,
                      "attributes": attributes, "valid": state in (VAR_ADDED, VAR_ADDED & 0xFE), "data": value})
        offset = following
    return found


def current(data: bytes) -> dict[str, bytes]:
    """Valid global variables by name."""
    return {v["name"]: v["data"] for v in variables(data) if v["valid"] and v["guid"] == GLOBAL}


def set_variable(data: bytearray, name: str, value: bytes, guid: uuid.UUID = GLOBAL,
                 attributes: int = NV_BS_RT) -> None:
    first, end, auth = _store(data)
    header = 60 if auth else 32
    existing = variables(data)
    offset = first
    if existing:
        offset = existing[-1]["next"]
    encoded = name.encode("utf-16-le") + b"\0\0"
    total = header + len(encoded) + len(value)
    padded = (total + 3) & ~3
    if offset + padded > end:
        raise VarsError("the variable store is full")
    if any(b != 0xFF for b in data[offset:offset + padded]):
        raise VarsError("the end of the variable store is not empty where it should be")
    for old in existing:
        if old["valid"] and old["name"] == name and old["guid"] == guid:
            data[old["offset"] + 2] &= VAR_DELETED
    if auth:
        head = struct.pack("<HBBIQ16sIII", 0x55AA, VAR_ADDED, 0, attributes, 0, b"\0" * 16, 0,
                           len(encoded), len(value)) + guid.bytes_le
    else:
        head = struct.pack("<HBBIII", 0x55AA, VAR_ADDED, 0, attributes, len(encoded), len(value)) + guid.bytes_le
    assert len(head) == header
    data[offset:offset + total] = head + encoded + value


def load_option(description: str, part_number: int, part_guid: str, first_lba: int, sectors: int,
                file_path: str = BOOTMGFW) -> bytes:
    """An EFI load option: HD(partition, GPT, GUID)/File(path)."""
    hard_drive = struct.pack("<BBHIQQ", 4, 1, 42, part_number, first_lba, sectors) \
        + uuid.UUID(part_guid).bytes_le + bytes([2, 2])
    name = file_path.encode("utf-16-le") + b"\0\0"
    file_node = struct.pack("<BBH", 4, 4, 4 + len(name)) + name
    path = hard_drive + file_node + struct.pack("<BBH", 0x7F, 0xFF, 4)
    return struct.pack("<IH", 1, len(path)) + description.encode("utf-16-le") + b"\0\0" + path


def add_windows_entry(data: bytearray, part_number: int, part_guid: str, first_lba: int, sectors: int,
                      description: str = "Windows Boot Manager (TUR guest)") -> str:
    """Add the entry and put it first in BootOrder. Returns its number as four hex digits."""
    now = current(data)
    used = {int(n[4:], 16) for n in now if len(n) == 8 and n.startswith("Boot")
            and all(c in "0123456789abcdefABCDEF" for c in n[4:])}
    number = next(n for n in range(0x00D0, 0x0100) if n not in used)
    set_variable(data, f"Boot{number:04X}", load_option(description, part_number, part_guid, first_lba, sectors))
    old = now.get("BootOrder", b"")
    order = [number] + [n for n in struct.unpack(f"<{len(old) // 2}H", old[:len(old) // 2 * 2]) if n != number]
    set_variable(data, "BootOrder", struct.pack(f"<{len(order)}H", *order))
    return f"{number:04X}"


def boot_entries(data: bytes) -> dict:
    """BootOrder and each entry's description and target, as text."""
    now = current(data)
    entries = {}
    for name, value in sorted(now.items()):
        if len(name) == 8 and name.startswith("Boot") and all(c in "0123456789abcdefABCDEF" for c in name[4:]) \
                and len(value) >= 8:
            path_length, = struct.unpack_from("<H", value, 4)
            end = 6
            while end + 1 < len(value) and value[end:end + 2] != b"\0\0":
                end += 2
            raw = value[end + 2:end + 2 + path_length]
            parts, at = [], 0
            while at + 4 <= len(raw):
                kind, sub, length = raw[at], raw[at + 1], struct.unpack_from("<H", raw, at + 2)[0]
                if length < 4 or kind == 0x7F:
                    break
                body = raw[at + 4:at + length]
                if kind == 4 and sub == 1 and len(body) >= 38:
                    parts.append(f"HD({struct.unpack_from('<I', body)[0]},GPT,{uuid.UUID(bytes_le=body[20:36])})")
                elif kind == 4 and sub == 4:
                    parts.append(body.decode("utf-16-le", "replace").rstrip("\0"))
                else:
                    parts.append(f"({kind},{sub})")
                at += length
            entries[name[4:].upper()] = {"description": value[6:end].decode("utf-16-le", "replace"),
                                         "path": "/".join(parts)}
    raw_order = now.get("BootOrder", b"")
    order = [f"{n:04X}" for n in struct.unpack(f"<{len(raw_order) // 2}H", raw_order[:len(raw_order) // 2 * 2])]
    return {"boot_order": order, "entries": entries}
