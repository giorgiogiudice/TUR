# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""A reader of the desktop's icon layout as Explorer writes it down
(HKCU\\Software\\Microsoft\\Windows\\Shell\\Bags\\1\\Desktop, value
IconLayouts), from the bytes alone. No published specification exists;
the structure below was read off the values of three runners in run
43552339780 (docs/reports/exp-inwin-1.md, section 7.3) and is checked in
every later run against the positions the desktop itself shows.

    16 bytes      zero
    4 x uint16    3, 1, 1, 1 on every runner seen (a version, presumably)
    uint64        n, the number of items
    n x           uint64 c, then c UTF-16 characters (the last one NUL):
                  the item's name in the Desktop folder (a shortcut's file
                  name, or "::{CLSID}" for a shell item such as the Recycle
                  Bin), then ">" and two flag characters
    ...           a block of fixed size (not decoded)
    uint32 n, 4 bytes, then n x (float32 column, float32 row, uint16 item)

Columns and rows are cells of the desktop's icon grid, counted from the
top left of the primary monitor's work area. Reads only.
"""

import struct


def decode(blob):
    """{"items": [{"name", "column", "row"}], "version", "table_at"}, or raises ValueError."""
    if len(blob) < 32:
        raise ValueError("too short")
    version = list(struct.unpack_from("<4H", blob, 16))
    count = struct.unpack_from("<Q", blob, 24)[0]
    if count > 10000:
        raise ValueError(f"implausible item count {count}")
    names = []
    offset = 32
    for _ in range(count):
        chars = struct.unpack_from("<Q", blob, offset)[0]
        offset += 8
        text = blob[offset:offset + 2 * chars].decode("utf-16-le", "replace").rstrip("\0")
        offset += 2 * chars
        name, _, flags = text.partition(">")
        names.append({"name": name, "flags": flags})
    # The position table: the item count again, then one entry per item
    # whose item numbers are exactly 0..n-1.
    for table in range(offset, len(blob) - 8):
        if struct.unpack_from("<I", blob, table)[0] != count:
            continue
        start = table + 8
        if start + 10 * count > len(blob):
            continue
        entries = [struct.unpack_from("<ffH", blob, start + 10 * i) for i in range(count)]
        if sorted(entry[2] for entry in entries) != list(range(count)):
            continue
        items = []
        for column, row, index in entries:
            items.append({"name": names[index]["name"], "flags": names[index]["flags"], "column": column, "row": row})
        return {"version": version, "items": items, "table_at": table, "bytes": len(blob)}
    raise ValueError("no position table found")


def compare(decoded, live, spacing, origin=(0, 0)):
    """How many icons the decoded layout puts in the cell the desktop shows
    them in. `live` is [{"name", "screen": [x, y, w, h]}] (UI Automation's
    names: shortcuts without ".lnk"); `spacing` is the grid's cell size in
    pixels (LVM_GETITEMSPACING)."""
    by_name = {}
    for item in decoded["items"]:
        name = item["name"]
        for suffix in (".lnk", ".url"):
            if name.lower().endswith(suffix):
                name = name[:-len(suffix)]
        by_name[name.lower()] = item
    result = {"live": len(live), "decoded": len(decoded["items"]), "matched_by_name": 0, "same_cell": 0, "different": []}
    unmatched = [icon for icon in live if (icon.get("name") or "").lower() not in by_name]
    shell_items = [item for item in decoded["items"] if item["name"].startswith("::")]
    for icon in live:
        item = by_name.get((icon.get("name") or "").lower())
        if item is None and len(unmatched) == 1 and len(shell_items) == 1 and icon is unmatched[0]:
            # A shell item (the Recycle Bin) is named by its class identifier
            # in the layout and by a translated name on the screen.
            item = shell_items[0]
            result["shell_item_paired"] = True
        if item is None or not icon.get("screen"):
            continue
        result["matched_by_name"] += 1
        x, y = icon["screen"][0] - origin[0], icon["screen"][1] - origin[1]
        cell = [round(x / spacing[0]), round(y / spacing[1])]
        if cell == [round(item["column"]), round(item["row"])]:
            result["same_cell"] += 1
        elif len(result["different"]) < 12:
            result["different"].append({"name": icon["name"], "shown_cell": cell, "decoded": [item["column"], item["row"]]})
    return result
