# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""A reader of Windows shortcut files (.lnk) that uses nothing of Windows:
the format is Microsoft's published open specification [MS-SHLLINK]. This
is what TUR's installer would use on the disk, before Windows ever runs;
the experiment compares it with what Windows itself says about the same
files (docs/reports/exp-inwin-1.md, section 7).

Also reads Internet shortcuts (.url), which are text.
"""

import struct

FLAGS = ("id_list", "link_info", "name", "relative_path", "working_dir", "arguments", "icon_location", "unicode")
SHOW = {1: "normal", 3: "maximised", 7: "minimised"}


def _ansi(data, offset):
    end = data.find(b"\0", offset)
    raw = data[offset:end if end >= 0 else len(data)]
    try:
        return raw.decode("mbcs", "replace")  # the system code page, on Windows
    except LookupError:
        return raw.decode("latin-1", "replace")


def _wide(data, offset):
    end = offset
    while end + 1 < len(data) and data[end:end + 2] != b"\0\0":
        end += 2
    return data[offset:end].decode("utf-16-le", "replace")


def parse_lnk(data):
    """The fields a desktop needs from a shortcut, or raises ValueError."""
    if len(data) < 76 or struct.unpack_from("<I", data, 0)[0] != 0x4C:
        raise ValueError("not a shell link")
    flags = struct.unpack_from("<I", data, 0x14)[0]
    has = {name: bool(flags >> bit & 1) for bit, name in enumerate(FLAGS)}
    result = {
        "icon_index": struct.unpack_from("<i", data, 0x38)[0],
        "show": SHOW.get(struct.unpack_from("<I", data, 0x3C)[0], "normal"),
        "hotkey": struct.unpack_from("<H", data, 0x40)[0],
        "has_id_list": has["id_list"],
    }
    offset = 0x4C
    if has["id_list"]:
        offset += 2 + struct.unpack_from("<H", data, offset)[0]
    if has["link_info"]:
        size, header, info_flags, _volume, local_base, _network, suffix = struct.unpack_from("<7I", data, offset)
        if info_flags & 1:
            target = _ansi(data, offset + local_base)
            if header >= 0x24:
                wide_base, wide_suffix = struct.unpack_from("<2I", data, offset + 0x1C)
                if wide_base:
                    target = _wide(data, offset + wide_base) + (_wide(data, offset + wide_suffix) if wide_suffix else "")
            elif suffix:
                target += _ansi(data, offset + suffix)
            result["target"] = target
        elif info_flags & 2:
            result["network"] = True
        offset += size
    for name in ("name", "relative_path", "working_dir", "arguments", "icon_location"):
        if not has[name]:
            continue
        count = struct.unpack_from("<H", data, offset)[0]
        offset += 2
        if has["unicode"]:
            result[name] = data[offset:offset + count * 2].decode("utf-16-le", "replace")
            offset += count * 2
        else:
            result[name] = data[offset:offset + count].decode("latin-1", "replace")
            offset += count
    blocks = []
    while offset + 8 <= len(data):
        size, signature = struct.unpack_from("<II", data, offset)
        if size < 4:
            break
        blocks.append(f"{signature:08x}")
        if signature == 0xA0000001 and "target" not in result and size >= 0x314:
            result["target"] = _wide(data, offset + 8 + 260)  # with %variables%
            result["target_has_variables"] = True
        if signature == 0xA0000007 and "icon_location" not in result and size >= 0x314:
            result["icon_location"] = _wide(data, offset + 8 + 260)
        if signature == 0xA0000006:
            result["installer_advertised"] = True  # the target is known only to Windows Installer
        offset += size
    result["blocks"] = blocks
    if "target" not in result:
        result["target_kind"] = "advertised" if result.get("installer_advertised") else ("shell item only" if has["id_list"] else "none")
    return result


def parse_url(text):
    """An Internet shortcut: the address and its icon."""
    result = {}
    section = ""
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("["):
            section = line.strip("[]").lower()
        elif "=" in line and section == "internetshortcut":
            key, value = line.split("=", 1)
            if key.lower() in ("url", "iconfile", "iconindex", "workingdirectory"):
                result[key.lower()] = value
    return result
