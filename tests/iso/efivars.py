#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Read the boot entries from an OVMF firmware-variable file, on the host.

    tests/iso/efivars.py OVMF_VARS.fd [OUT.json]

Prints, and writes as JSON, every `Boot####` variable (its description, its
device path as text) and `BootOrder`, as the file holds them after the
virtual machine stopped. It reads the file only. The test uses it to show
what an installation and a first start did to the firmware's boot entries,
without trusting anything inside the guest.

The format is the one edk2 writes (MdeModulePkg/Include/Guid/VariableFormat.h):
a firmware volume, a variable store header, then variables, each starting
with 0x55AA, with a state byte that tells added from deleted ones. Both the
authenticated store (the one OVMF uses) and the plain one are read.
"""

from __future__ import annotations

import json
import pathlib
import struct
import sys
import uuid

GLOBAL = uuid.UUID("8be4df61-93ca-11d2-aa0d-00e098032b8c")
AUTH_STORE = uuid.UUID("aaf32c78-947b-439a-a180-2e144ec37792")
PLAIN_STORE = uuid.UUID("fff12b8d-7696-4c8b-a985-2747075b4f50")
VAR_ADDED = 0x3F
# A variable being replaced is first marked "in deleted transition"; it is
# still the current value until the new one is complete.
VAR_IN_DELETED_TRANSITION = 0xFE


def find_store(data: bytes) -> tuple[int, bool]:
    """Offset of the first variable, and whether the store is authenticated."""
    for guid, auth in ((AUTH_STORE, True), (PLAIN_STORE, False)):
        at = data.find(guid.bytes_le)
        if at >= 0:
            # GUID, Size (4), Format (1), State (1), Reserved (2), Reserved1 (4).
            return at + 28, auth
    raise ValueError("no variable store found")


def variables(data: bytes) -> list[dict]:
    offset, auth = find_store(data)
    header = 60 if auth else 32
    found = []
    while offset + header <= len(data):
        if data[offset:offset + 2] != b"\xaa\x55":
            break
        state = data[offset + 2]
        attributes = struct.unpack_from("<I", data, offset + 4)[0]
        if auth:
            name_size, data_size = struct.unpack_from("<II", data, offset + 36)
            guid = uuid.UUID(bytes_le=data[offset + 44:offset + 60])
        else:
            name_size, data_size = struct.unpack_from("<II", data, offset + 8)
            guid = uuid.UUID(bytes_le=data[offset + 16:offset + 32])
        name_at = offset + header
        name = data[name_at:name_at + name_size].decode("utf-16-le", errors="replace").rstrip("\0")
        value = data[name_at + name_size:name_at + name_size + data_size]
        valid = state == VAR_ADDED or state == (VAR_ADDED & VAR_IN_DELETED_TRANSITION)
        found.append({"name": name, "guid": str(guid), "state": state, "valid": valid,
                      "attributes": attributes, "data": value})
        offset = name_at + name_size + data_size
        offset = (offset + 3) & ~3
    return found


def device_path(raw: bytes) -> str:
    """The parts of a device path the test cares about, as text."""
    parts, at = [], 0
    while at + 4 <= len(raw):
        kind, sub, length = raw[at], raw[at + 1], struct.unpack_from("<H", raw, at + 2)[0]
        if length < 4:
            break
        body = raw[at + 4:at + length]
        if kind == 0x7F:
            break
        if kind == 4 and sub == 1 and len(body) >= 38:
            number, start, size = struct.unpack_from("<IQQ", body, 0)
            signature = uuid.UUID(bytes_le=body[20:36])
            parts.append(f"HD({number},GPT,{signature},{start:#x},{size:#x})")
        elif kind == 4 and sub == 4:
            parts.append(body.decode("utf-16-le", errors="replace").rstrip("\0"))
        elif kind == 4 and sub == 6:
            parts.append(f"FvFile({uuid.UUID(bytes_le=body[:16])})")
        elif kind == 4 and sub == 7:
            parts.append(f"Fv({uuid.UUID(bytes_le=body[:16])})")
        elif kind == 1 and sub == 1 and len(body) >= 2:
            parts.append(f"Pci({body[1]:#x},{body[0]:#x})")
        elif kind == 2 and sub == 1:
            parts.append("PciRoot")
        elif kind == 3:
            parts.append(f"Msg({sub})")
        else:
            parts.append(f"({kind},{sub})")
        at += length
    return "/".join(parts)


def boot_entries(path: pathlib.Path) -> dict:
    data = path.read_bytes()
    current = {}
    for var in variables(data):
        if var["valid"] and uuid.UUID(var["guid"]) == GLOBAL:
            current[var["name"]] = var["data"]
    entries = {}
    for name, value in sorted(current.items()):
        if len(name) == 8 and name.startswith("Boot") and all(c in "0123456789ABCDEFabcdef" for c in name[4:]):
            attributes, path_length = struct.unpack_from("<IH", value, 0)
            end = 6
            while end + 1 < len(value) and value[end:end + 2] != b"\0\0":
                end += 2
            description = value[6:end].decode("utf-16-le", errors="replace")
            dp = value[end + 2:end + 2 + path_length]
            entries[name[4:].upper()] = {
                "description": description,
                "active": bool(attributes & 1),
                "device_path": device_path(dp),
            }
    order_raw = current.get("BootOrder", b"")
    order = [f"{number:04X}" for number in struct.unpack(f"<{len(order_raw) // 2}H", order_raw)] if order_raw else []
    return {
        "file": str(path),
        "boot_order": order,
        "boot_order_descriptions": [entries.get(number, {}).get("description") for number in order],
        "entries": entries,
    }


def main() -> int:
    if len(sys.argv) not in (2, 3):
        print(__doc__, file=sys.stderr)
        return 2
    result = boot_entries(pathlib.Path(sys.argv[1]))
    text = json.dumps(result, indent=2)
    print(text)
    if len(sys.argv) == 3:
        pathlib.Path(sys.argv[2]).write_text(text + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
