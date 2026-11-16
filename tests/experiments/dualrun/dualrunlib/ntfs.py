# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""What state did Windows leave an NTFS volume in? Read-only, no mount.

The Linux `ntfs3` driver has no hibernation check (docs/architecture/
storage.md, section 8), and a read/write mount attempt can write before it
refuses. So the state is read straight from the volume, opened read-only:

- the boot sector: NTFS, or BitLocker (`-FVE-FS-`), or something else;
- `$Volume` (file record 3): the label, the NTFS version and the volume
  flags (bit 0 = dirty: Windows wants its disk check);
- `$LogFile` (file record 2): the two restart pages. The journal is "clean"
  when the newer restart area has the RESTART_VOLUME_IS_CLEAN flag or no
  client in use (the rule ntfs-3g uses); otherwise Windows did not unmount
  the volume: a power cut, a crash, a kill, or hibernation;
- `hiberfil.sys` in the root directory: its first bytes. `hibr`/`HIBR` is a
  valid hibernation image (hibernation, or the kernel image Fast Startup
  leaves); `wake`/`WAKE` or a zeroed header is a used one.

Anything that cannot be read or understood gives `None` for that fact, and
the check-list treats `None` as "unknown", which is never safe.
"""

from __future__ import annotations

import struct

from .blockio import Dev

VOLUME_DIRTY = 0x0001
RESTART_VOLUME_IS_CLEAN = 0x0002
NO_CLIENT = 0xFFFF


class NtfsError(Exception):
    """The volume could not be read as NTFS."""


def signature(dev: Dev) -> str:
    """What the first sector says the partition holds."""
    if dev.size < 512:
        return "empty"
    boot = dev.pread(0, 512)
    oem = boot[3:11]
    if oem == b"NTFS    ":
        return "ntfs"
    if oem == b"-FVE-FS-":
        return "bitlocker"
    if oem == b"EXFAT   ":
        return "exfat"
    if boot[3:7] == b"ReFS":
        return "refs"
    if boot[510:512] == b"\x55\xaa" and (boot[82:87] == b"FAT32" or boot[54:57] == b"FAT"):
        return "fat"
    if boot == b"\0" * 512:
        return "blank"
    return "unknown"


def _fixup(record: bytearray, what: str) -> bytearray:
    """Undo the update-sequence protection of a multi-sector record."""
    usa_offset, usa_count = struct.unpack_from("<HH", record, 4)
    if usa_count < 1 or usa_offset + 2 * usa_count > len(record) or (usa_count - 1) * 512 > len(record):
        raise NtfsError(f"{what}: bad update sequence array")
    number = record[usa_offset:usa_offset + 2]
    for i in range(1, usa_count):
        end = i * 512
        if record[end - 2:end] != number:
            raise NtfsError(f"{what}: a sector was not written completely (torn write)")
        record[end - 2:end] = record[usa_offset + 2 * i:usa_offset + 2 * i + 2]
    return record


def _runs(data: bytes) -> list[tuple[int, int | None]]:
    """Decode a run list into (cluster count, first cluster or None for a hole)."""
    runs, pos, lcn = [], 0, 0
    while pos < len(data) and data[pos] != 0:
        header = data[pos]
        length_size, offset_size = header & 0x0F, header >> 4
        pos += 1
        if length_size == 0 or pos + length_size + offset_size > len(data):
            raise NtfsError("bad run list")
        count = int.from_bytes(data[pos:pos + length_size], "little")
        pos += length_size
        if offset_size:
            lcn += int.from_bytes(data[pos:pos + offset_size], "little", signed=True)
            pos += offset_size
            runs.append((count, lcn))
        else:
            runs.append((count, None))
    return runs


class Attribute:
    def __init__(self, record: bytes, offset: int):
        self.offset = offset
        self.type, self.length = struct.unpack_from("<II", record, offset)
        self.non_resident = record[offset + 8]
        name_length, name_offset = record[offset + 9], struct.unpack_from("<H", record, offset + 10)[0]
        self.name = record[offset + name_offset:offset + name_offset + 2 * name_length].decode("utf-16-le", "replace")
        self.raw = record[offset:offset + self.length]
        if self.non_resident:
            self.start_vcn, = struct.unpack_from("<Q", self.raw, 16)
            run_offset, = struct.unpack_from("<H", self.raw, 32)
            self.real_size, = struct.unpack_from("<Q", self.raw, 48)
            self.runs = _runs(self.raw[run_offset:])
            self.value = None
        else:
            size, value_offset = struct.unpack_from("<IH", self.raw, 16)
            self.value_offset = offset + value_offset
            self.value = self.raw[value_offset:value_offset + size]
            self.real_size = size
            self.runs = []


class Ntfs:
    def __init__(self, dev: Dev):
        self.dev = dev
        boot = dev.pread(0, 512)
        if boot[3:11] != b"NTFS    ":
            raise NtfsError("not an NTFS boot sector")
        self.bytes_per_sector, = struct.unpack_from("<H", boot, 0x0B)
        spc = boot[0x0D]
        sectors_per_cluster = spc if spc <= 0x80 else 1 << (256 - spc)
        self.cluster = self.bytes_per_sector * sectors_per_cluster
        self.total_sectors, mft_lcn = struct.unpack_from("<QQ", boot, 0x28)
        self.serial = boot[0x48:0x50].hex()
        size_code = struct.unpack_from("<b", boot, 0x40)[0]
        self.record_size = size_code * self.cluster if size_code > 0 else 1 << -size_code
        if self.bytes_per_sector not in (512, 1024, 2048, 4096) or not 512 <= self.cluster <= 2 * 1024 * 1024 \
                or self.record_size not in (1024, 2048, 4096) or mft_lcn * self.cluster >= dev.size:
            raise NtfsError("the boot sector's numbers are not plausible")
        self.mft_offset = mft_lcn * self.cluster
        self._mft_runs: list[tuple[int, int | None]] | None = None

    # -- file records -----------------------------------------------------

    def _parse_record(self, raw: bytes, number: int) -> tuple[int, list[Attribute]]:
        record = bytearray(raw)
        if record[:4] != b"FILE":
            raise NtfsError(f"file record {number} has no FILE signature")
        _fixup(record, f"file record {number}")
        attrs_offset, flags = struct.unpack_from("<HH", record, 20)
        attributes, pos = [], attrs_offset
        while pos + 8 <= len(record):
            kind, length = struct.unpack_from("<II", record, pos)
            if kind == 0xFFFFFFFF:
                break
            if length < 16 or pos + length > len(record):
                raise NtfsError(f"file record {number}: bad attribute length")
            attributes.append(Attribute(bytes(record), pos))
            pos += length
        return flags, attributes

    def record(self, number: int) -> tuple[int, list[Attribute]]:
        """(flags, attributes) of file record `number`."""
        if number < 16:
            raw = self.dev.pread(self.mft_offset + number * self.record_size, self.record_size)
        else:
            if self._mft_runs is None:
                _, attrs = self.record(0)
                data = [a for a in attrs if a.type == 0x80 and not a.name and a.non_resident and a.start_vcn == 0]
                if not data:
                    raise NtfsError("$MFT has no readable run list")
                self._mft_runs = data[0].runs
            raw = self.read_runs(self._mft_runs, number * self.record_size, self.record_size)
        return self._parse_record(raw, number)

    def read_runs(self, runs: list[tuple[int, int | None]], offset: int, length: int) -> bytes:
        out, position = bytearray(), 0
        for count, lcn in runs:
            span = count * self.cluster
            if offset < position + span and len(out) < length:
                inside = max(offset, position) - position
                take = min(span - inside, length - len(out))
                if lcn is None:
                    out += b"\0" * take
                else:
                    out += self.dev.pread(lcn * self.cluster + inside, take)
            position += span
            if len(out) >= length:
                break
        if len(out) != length:
            raise NtfsError("the data lies outside the run list held in the base file record")
        return bytes(out)

    def read_attribute(self, attribute: Attribute, offset: int, length: int) -> bytes:
        length = max(0, min(length, attribute.real_size - offset))
        if attribute.non_resident:
            return self.read_runs(attribute.runs, offset, length)
        return attribute.value[offset:offset + length]

    # -- the facts --------------------------------------------------------

    def volume(self) -> dict:
        _, attrs = self.record(3)
        out: dict = {"label": "", "flags": None, "version": None}
        for a in attrs:
            if a.type == 0x60 and a.value is not None:
                out["label"] = a.value.decode("utf-16-le", "replace")
            elif a.type == 0x70 and a.value is not None and len(a.value) >= 12:
                major, minor, flags = struct.unpack_from("<BBH", a.value, 8)
                out["version"] = f"{major}.{minor}"
                out["flags"] = flags
        return out

    def journal(self) -> dict:
        """The restart pages of $LogFile and whether they say the volume was unmounted."""
        _, attrs = self.record(2)
        data = [a for a in attrs if a.type == 0x80 and not a.name]
        if not data:
            raise NtfsError("$LogFile has no data")
        size = data[0].real_size
        pages = []
        first = self.read_attribute(data[0], 0, min(size, 4096))
        page_size = 4096
        if first[:4] == b"RSTR":
            page_size = struct.unpack_from("<I", first, 16)[0] or 4096
        if page_size not in (512, 1024, 2048, 4096, 8192, 16384, 32768, 65536):
            raise NtfsError("$LogFile: implausible page size")
        for index in (0, 1):
            raw = self.read_attribute(data[0], index * page_size, page_size)
            magic = raw[:4]
            page: dict = {"magic": "".join(chr(b) if 32 <= b < 127 else "." for b in magic)}
            if magic == b"RSTR":
                try:
                    fixed = _fixup(bytearray(raw), "restart page")
                    area_offset, minor, major = struct.unpack_from("<Hhh", fixed, 24)
                    current_lsn, _clients, _free, in_use, flags = struct.unpack_from("<QHHHH", fixed, area_offset)
                    page.update(version=f"{major}.{minor}", current_lsn=current_lsn, client_in_use=in_use,
                                flags=flags, clean=bool(flags & RESTART_VOLUME_IS_CLEAN) or in_use == NO_CLIENT)
                except (NtfsError, struct.error) as error:
                    page["error"] = str(error)
            elif raw.count(0xFF) == len(raw):
                page.update(magic="empty", clean=True)
            pages.append(page)
        valid = [p for p in pages if p.get("magic") == "RSTR" and "clean" in p]
        if valid:
            newest = max(valid, key=lambda p: p["current_lsn"])
            clean = newest["clean"]
        elif all(p["magic"] == "empty" for p in pages):
            clean = True       # a journal never used (a volume made by mkfs.ntfs)
        else:
            clean = None       # CHKD, BAAD or damage: not understood
        return {"clean": clean, "pages": pages}

    def root_entries(self) -> dict[str, int]:
        """Names in the root directory (lower case) and their file record numbers."""
        _, attrs = self.record(5)
        names: dict[str, int] = {}

        def walk(block: bytes, header_at: int) -> None:
            entries_offset, index_length = struct.unpack_from("<II", block, header_at)
            pos, end = header_at + entries_offset, min(len(block), header_at + index_length)
            while pos + 16 <= end:
                reference, length, key_length, flags = struct.unpack_from("<QHHH", block, pos)
                if length < 16:
                    break
                if key_length >= 66 and not flags & 2:
                    name_length = block[pos + 16 + 64]
                    raw = block[pos + 16 + 66:pos + 16 + 66 + 2 * name_length]
                    names[raw.decode("utf-16-le", "replace").lower()] = reference & 0xFFFFFFFFFFFF
                if flags & 2:
                    break
                pos += length

        root = [a for a in attrs if a.type == 0x90 and a.name == "$I30" and a.value is not None]
        if not root:
            raise NtfsError("the root directory has no index root in its base file record")
        block_size, = struct.unpack_from("<I", root[0].value, 8)
        walk(root[0].value, 16)
        for a in attrs:
            if a.type == 0xA0 and a.name == "$I30" and a.non_resident:
                if a.start_vcn != 0 or block_size not in (512, 1024, 2048, 4096, 8192, 16384, 32768, 65536):
                    raise NtfsError("the root directory index is in a form this program does not read")
                total = sum(count for count, _ in a.runs) * self.cluster
                # Which index blocks are in use: the directory's own bitmap. A block that is
                # not in use may hold anything; one that is in use must read correctly.
                bitmaps = [b for b in attrs if b.type == 0xB0 and b.name == "$I30"]
                bitmap = self.read_attribute(bitmaps[0], 0, bitmaps[0].real_size) if bitmaps else None
                for number, offset in enumerate(range(0, total - block_size + 1, block_size)):
                    if bitmap is not None:
                        if number // 8 >= len(bitmap) or not bitmap[number // 8] >> (number % 8) & 1:
                            continue
                    raw = bytearray(self.read_runs(a.runs, offset, block_size))
                    if raw[:4] != b"INDX":
                        if bitmap is not None:
                            raise NtfsError("an index block of the root directory that is in use is not readable")
                        continue
                    try:
                        walk(bytes(_fixup(raw, "index block")), 24)
                    except (NtfsError, struct.error):
                        raise NtfsError("an index block of the root directory is damaged")
        if any(a.type == 0x20 for a in attrs) and not any(a.type == 0xA0 for a in attrs) and len(names) < 3:
            raise NtfsError("the root directory's index is listed outside its base file record")
        return names

    def hiberfil(self) -> dict:
        """Presence, size and first bytes of \\hiberfil.sys."""
        number = self.root_entries().get("hiberfil.sys")
        if number is None:
            return {"present": False, "state": "none"}
        flags, attrs = self.record(number)
        named = False
        for a in attrs:
            if a.type == 0x30 and a.value is not None and len(a.value) >= 66:
                parent = struct.unpack_from("<Q", a.value, 0)[0] & 0xFFFFFFFFFFFF
                name = a.value[66:66 + 2 * a.value[64]].decode("utf-16-le", "replace").lower()
                if parent == 5 and name == "hiberfil.sys":
                    named = True
        if not flags & 1 or not named:
            return {"present": False, "state": "none", "note": "a stale directory entry"}
        data = [a for a in attrs if a.type == 0x80 and not a.name]
        if not data:
            if any(a.type == 0x20 for a in attrs):
                raise NtfsError("hiberfil.sys keeps its data outside its base file record")
            return {"present": True, "size": 0, "state": "empty"}
        if data[0].non_resident and data[0].start_vcn != 0:
            raise NtfsError("hiberfil.sys: the first part of its data is not in its base file record")
        size = data[0].real_size
        head = self.read_attribute(data[0], 0, min(size, 4096))
        magic = head[:4]
        if size == 0 or not head.strip(b"\0"):
            state = "empty"
        elif magic.lower() == b"hibr":
            state = "hibernated"
        elif magic.lower() in (b"wake", b"rstr"):
            state = "used"
        else:
            state = "unknown"
        return {"present": True, "size": size, "state": state, "magic": magic.hex(),
                "magic_text": "".join(chr(b) if 32 <= b < 127 else "." for b in magic),
                "head_hex": head[:64].hex()}


def probe(dev: Dev) -> dict:
    """Every fact the check-list needs about one partition. Never raises."""
    out: dict = {"kind": "unknown"}
    try:
        out["kind"] = signature(dev)
    except (OSError, ValueError) as error:
        out["error"] = f"cannot read the first sector: {error}"
        return out
    if out["kind"] != "ntfs":
        return out
    facts: dict = {"dirty": None, "journal_clean": None, "hibernation": None}
    out["ntfs"] = facts
    try:
        fs = Ntfs(dev)
        facts["serial"] = fs.serial
        facts["cluster_bytes"] = fs.cluster
    except (NtfsError, OSError, ValueError, struct.error) as error:
        facts["error"] = str(error)
        return out
    for name, call in (("volume", fs.volume), ("journal", fs.journal), ("hiberfil", fs.hiberfil),
                       ("root", fs.root_entries)):
        try:
            value = call()
        except (NtfsError, OSError, ValueError, struct.error, IndexError) as error:
            facts.setdefault("errors", {})[name] = str(error)
            continue
        if name == "volume":
            out["label"] = value["label"]
            facts["version"] = value["version"]
            facts["volume_flags"] = value["flags"]
            facts["dirty"] = None if value["flags"] is None else bool(value["flags"] & VOLUME_DIRTY)
        elif name == "journal":
            facts["journal"] = value
            facts["journal_clean"] = value["clean"]
        elif name == "hiberfil":
            facts["hiberfil"] = value
            facts["hibernation"] = value["state"]
        else:
            facts["has_windows_directory"] = "windows" in value
            facts["root_names"] = len(value)
    return out
