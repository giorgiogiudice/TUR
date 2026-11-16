# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Readers for the binary formats the detector looks into. Nothing here
executes or loads the file it reads.

  - executables: PE (sections, overlay, CLR header), ELF, Mach-O (thin, fat)
  - Electron's asar archive: header reader, and a writer that keeps the data
    section and rewrites only the header (so files can be added or replaced)
  - Electron fuses
  - Godot's PCK header and directory, embedded or standalone
  - PyInstaller's CArchive and PYZ
  - .NET's single-file bundle
  - Apple's pbzx stream (inside .pkg payloads)
"""

import io
import json
import lzma
import marshal
import mmap
import os
import re
import struct
import zlib

# ---------------------------------------------------------------- executables

PE_MACHINES = {0x8664: "x86_64", 0x14C: "x86", 0xAA64: "arm64", 0x1C4: "arm"}
ELF_MACHINES = {62: "x86_64", 3: "x86", 183: "arm64", 40: "arm", 243: "riscv"}
MACHO_CPUS = {0x01000007: "x86_64", 0x0100000C: "arm64", 7: "x86", 12: "arm", 18: "ppc", 0x01000012: "ppc64"}


def read_at(path, offset, size):
    with open(path, "rb") as handle:
        handle.seek(offset)
        return handle.read(size)


def pe_info(path):
    """Machine, sections, where the overlay starts, CLR header, subsystem.
    None when the file is not a PE image."""
    try:
        size = os.path.getsize(path)
        with open(path, "rb") as handle:
            head = handle.read(4096)
            if len(head) < 64 or head[:2] != b"MZ":
                return None
            lfanew = struct.unpack_from("<I", head, 0x3C)[0]
            if lfanew + 24 > size:
                return None
            handle.seek(lfanew)
            header = handle.read(24)
            if header[:4] != b"PE\0\0":
                return None
            machine, nsections, _, _, _, opt_size, characteristics = struct.unpack_from("<HHIIIHH", header, 4)
            optional = handle.read(opt_size)
            magic = struct.unpack_from("<H", optional, 0)[0] if len(optional) >= 2 else 0
            plus = magic == 0x20B
            subsystem = struct.unpack_from("<H", optional, 68)[0] if len(optional) >= 70 else 0
            directories = 112 if plus else 96
            clr = False
            if len(optional) >= directories + 8 * 15:
                rva, length = struct.unpack_from("<II", optional, directories + 8 * 14)
                clr = rva != 0 and length != 0
            security = (0, 0)
            if len(optional) >= directories + 8 * 5:
                security = struct.unpack_from("<II", optional, directories + 8 * 4)
            sections = []
            end = 0
            table = handle.read(40 * nsections)
            for index in range(nsections):
                entry = table[index * 40 : index * 40 + 40]
                if len(entry) < 40:
                    break
                name = entry[:8].rstrip(b"\0").decode("latin-1")
                vsize, vaddr, raw_size, raw_ptr = struct.unpack_from("<IIII", entry, 8)
                sections.append({"name": name, "raw": raw_ptr, "size": raw_size, "vaddr": vaddr, "vsize": vsize})
                end = max(end, raw_ptr + raw_size)
        return {
            "machine": PE_MACHINES.get(machine, hex(machine)),
            "dll": bool(characteristics & 0x2000),
            "plus": plus,
            "subsystem": subsystem,
            "clr": clr,
            "sections": sections,
            "overlay": end,
            "overlay_size": max(0, size - end),
            "signature": security,  # file offset and size of the Authenticode blob
            "size": size,
        }
    except (OSError, struct.error):
        return None


def binary_kind(path):
    """("pe", [arch]) / ("elf", [arch]) / ("macho", [archs]) / None."""
    try:
        head = read_at(path, 0, 4096)
    except OSError:
        return None
    if len(head) < 20:
        return None
    if head[:2] == b"MZ":
        info = pe_info(path)
        return ("pe", [info["machine"]]) if info else None
    if head[:4] == b"\x7fELF":
        order = "<" if head[5] == 1 else ">"
        machine = struct.unpack_from(order + "H", head, 18)[0]
        return ("elf", [ELF_MACHINES.get(machine, str(machine))])
    magic = head[:4]
    if magic in (b"\xcf\xfa\xed\xfe", b"\xce\xfa\xed\xfe"):
        cpu = struct.unpack_from("<I", head, 4)[0]
        return ("macho", [MACHO_CPUS.get(cpu, hex(cpu))])
    if magic in (b"\xfe\xed\xfa\xcf", b"\xfe\xed\xfa\xce"):
        cpu = struct.unpack_from(">I", head, 4)[0]
        return ("macho", [MACHO_CPUS.get(cpu, hex(cpu))])
    if magic in (b"\xca\xfe\xba\xbe", b"\xca\xfe\xba\xbf"):
        count = struct.unpack_from(">I", head, 4)[0]
        if count > 44:  # a Java class file shares the magic
            return None
        step = 20 if magic == b"\xca\xfe\xba\xbe" else 32
        archs = []
        for index in range(count):
            cpu = struct.unpack_from(">I", head, 8 + index * step)[0]
            archs.append(MACHO_CPUS.get(cpu, hex(cpu)))
        return ("macho", archs)
    return None


def scan(path, pattern, limit=40, window=None):
    """All distinct matches of a bytes regex in a file, through mmap."""
    found = []
    try:
        if os.path.getsize(path) == 0:
            return found
        with open(path, "rb") as handle, mmap.mmap(handle.fileno(), 0, access=mmap.ACCESS_READ) as view:
            for match in re.finditer(pattern, view):
                value = match.group(1) if match.groups() else match.group(0)
                if value not in found:
                    found.append(value)
                    if len(found) >= limit:
                        break
    except (OSError, ValueError):
        pass
    return [value.decode("latin-1") for value in found]


def contains(path, needle):
    try:
        if os.path.getsize(path) == 0:
            return False
        with open(path, "rb") as handle, mmap.mmap(handle.fileno(), 0, access=mmap.ACCESS_READ) as view:
            return view.find(needle) != -1
    except (OSError, ValueError):
        return False


def pe_version_strings(path):
    """FileVersion, ProductVersion, ProductName... from a PE version resource,
    found by their UTF-16 keys (no resource tree walk)."""
    out = {}
    try:
        with open(path, "rb") as handle, mmap.mmap(handle.fileno(), 0, access=mmap.ACCESS_READ) as view:
            for key in ("FileVersion", "ProductVersion", "ProductName", "FileDescription", "CompanyName", "OriginalFilename", "InternalName"):
                needle = (key + "\0").encode("utf-16-le")
                at = view.rfind(needle)
                if at == -1:
                    continue
                start = at + len(needle)
                start += (4 - (start % 4)) % 4  # values are 32-bit aligned
                raw = view[start : start + 512]
                end = raw.find(b"\0\0\0")
                if end == -1:
                    continue
                if end % 2:
                    end += 1
                try:
                    text = raw[:end].decode("utf-16-le").strip("\0 ")
                except UnicodeDecodeError:
                    continue
                if text and all(ch.isprintable() for ch in text):
                    out[key] = text[:120]
    except (OSError, ValueError):
        pass
    return out


# ------------------------------------------------------------------- zip tail


def zip_tail(path):
    """Where a zip archive appended to another file starts and ends (a
    launch4j jar, a fused LÖVE game, a self-extracting zip). None if the
    file does not end in one. An Authenticode signature after the archive is
    allowed for."""
    try:
        size = os.path.getsize(path)
        tail_size = min(size, 1 << 20)
        tail = read_at(path, size - tail_size, tail_size)
    except OSError:
        return None
    at = tail.rfind(b"PK\x05\x06")
    while at != -1:
        if at + 22 <= len(tail):
            entries, cd_size, cd_offset, comment = struct.unpack_from("<HIIH", tail, at + 10)
            eocd = size - tail_size + at
            start = eocd - cd_size - cd_offset
            if entries and 0 <= start < eocd and cd_size < size:
                if read_at(path, start + cd_offset, 4) == b"PK\x01\x02":
                    return {"start": start, "end": eocd + 22 + comment, "entries": entries}
        at = tail.rfind(b"PK\x05\x06", 0, at)
    return None


# ----------------------------------------------------------------------- asar


class Asar:
    """Electron's archive: a JSON header, then every packed file end to end.
    Files flagged "unpacked" live beside it in <name>.unpacked/."""

    def __init__(self, path):
        self.path = path
        with open(path, "rb") as handle:
            head = handle.read(16)
            if len(head) < 16:
                raise ValueError("not an asar archive: too short")
            four, header_size, _payload, json_size = struct.unpack("<4I", head)
            if four != 4 or json_size > header_size or header_size > os.path.getsize(path):
                raise ValueError("not an asar archive")
            self.header = json.loads(handle.read(json_size).decode("utf-8"))
        self.data_offset = 8 + header_size
        self.unpacked_dir = path + ".unpacked"
        self.appended = []  # (bytes) added to the end of the data section
        self.data_size = os.path.getsize(path) - self.data_offset

    def walk(self, node=None, prefix=""):
        node = self.header if node is None else node
        for name, child in node.get("files", {}).items():
            path = prefix + name
            if "files" in child:
                yield path, child, True
                yield from self.walk(child, path + "/")
            else:
                yield path, child, False

    def node(self, relpath):
        node = self.header
        for part in relpath.strip("/").split("/"):
            node = node.get("files", {}).get(part)
            if node is None:
                return None
        return node

    def read(self, relpath, limit=64 << 20):
        node = self.node(relpath)
        if node is None or "files" in node or "link" in node:
            return None
        if node.get("unpacked"):
            try:
                with open(os.path.join(self.unpacked_dir, relpath), "rb") as handle:
                    return handle.read(limit)
            except OSError:
                return None
        offset = int(node["offset"])
        size = min(int(node["size"]), limit)
        if offset >= self.data_size:  # appended in this session
            start = offset - self.data_size
            blob = b"".join(self.appended)
            return blob[start : start + size]
        return read_at(self.path, self.data_offset + offset, size)

    def read_json(self, relpath):
        raw = self.read(relpath, 4 << 20)
        if raw is None:
            return None
        try:
            return json.loads(raw.decode("utf-8-sig"))
        except (ValueError, UnicodeDecodeError):
            return None

    # -- changes: the data section is kept, new bytes go after it

    def _parent(self, relpath, create=True):
        parts = relpath.strip("/").split("/")
        node = self.header
        for part in parts[:-1]:
            files = node.setdefault("files", {})
            if part not in files:
                if not create:
                    return None, parts[-1]
                files[part] = {"files": {}}
            node = files[part]
        return node, parts[-1]

    def put_packed(self, relpath, data):
        """Add or replace a file inside the archive."""
        parent, name = self._parent(relpath)
        offset = self.data_size + sum(len(blob) for blob in self.appended)
        self.appended.append(data)
        parent.setdefault("files", {})[name] = {"size": len(data), "offset": str(offset)}

    def put_unpacked(self, relpath, data, executable=False):
        """Add or replace a file that lives in <asar>.unpacked/ (what a
        native module must be: the loader needs a real file)."""
        parent, name = self._parent(relpath)
        entry = {"size": len(data), "unpacked": True}
        if executable:
            entry["executable"] = True
        parent.setdefault("files", {})[name] = entry
        target = os.path.join(self.unpacked_dir, relpath)
        os.makedirs(os.path.dirname(target), exist_ok=True)
        with open(target, "wb") as handle:
            handle.write(data)
        if executable:
            os.chmod(target, 0o755)

    def save(self, dest=None):
        """Write the archive with the current header. The file data is
        copied as it is; integrity records of changed files are dropped
        (Electron checks them on macOS and Windows only)."""
        dest = dest or self.path
        blob = json.dumps(self.header, separators=(",", ":")).encode("utf-8")
        padded = len(blob) + (4 - len(blob) % 4) % 4
        head = struct.pack("<4I", 4, padded + 8, padded + 4, len(blob)) + blob + b"\0" * (padded - len(blob))
        temporary = dest + ".tur-new"
        with open(self.path, "rb") as source, open(temporary, "wb") as out:
            out.write(head)
            source.seek(self.data_offset)
            while True:
                chunk = source.read(4 << 20)
                if not chunk:
                    break
                out.write(chunk)
            for extra in self.appended:
                out.write(extra)
        os.replace(temporary, dest)
        self.path = dest
        self.data_offset = len(head)
        self.data_size = os.path.getsize(dest) - self.data_offset
        self.appended = []


def write_asar(dest, files):
    """A new asar from {relative path: bytes} (used by the self-test)."""
    header = {"files": {}}
    blobs = []
    offset = 0
    for relpath, data in files.items():
        node = header
        parts = relpath.split("/")
        for part in parts[:-1]:
            node = node["files"].setdefault(part, {"files": {}})
        node["files"][parts[-1]] = {"size": len(data), "offset": str(offset)}
        blobs.append(data)
        offset += len(data)
    blob = json.dumps(header, separators=(",", ":")).encode("utf-8")
    padded = len(blob) + (4 - len(blob) % 4) % 4
    with open(dest, "wb") as out:
        out.write(struct.pack("<4I", 4, padded + 8, padded + 4, len(blob)) + blob + b"\0" * (padded - len(blob)))
        for data in blobs:
            out.write(data)


# --------------------------------------------------------------------- fuses

FUSE_SENTINEL = b"dL7pKGdnNz796PbbjQWNKmHXBZaB9tsX"
FUSE_NAMES = [
    "RunAsNode",
    "EnableCookieEncryption",
    "EnableNodeOptionsEnvironmentVariable",
    "EnableNodeCliInspectArguments",
    "EnableEmbeddedAsarIntegrityValidation",
    "OnlyLoadAppFromAsar",
    "LoadBrowserProcessSpecificV8Snapshot",
    "GrantFileProtocolExtraPrivileges",
]


def read_fuses(path):
    """Electron's fuse wire: {name: "0"|"1"|"r"} or None."""
    try:
        with open(path, "rb") as handle, mmap.mmap(handle.fileno(), 0, access=mmap.ACCESS_READ) as view:
            at = view.find(FUSE_SENTINEL)
            if at == -1:
                return None
            start = at + len(FUSE_SENTINEL)
            version, length = view[start], view[start + 1]
            if version != 1 or length > 32:
                return None
            states = view[start + 2 : start + 2 + length].decode("latin-1")
    except (OSError, ValueError, IndexError):
        return None
    return {(FUSE_NAMES[index] if index < len(FUSE_NAMES) else f"fuse{index}"): state for index, state in enumerate(states)}


def write_fuses(path, wanted):
    """Set fuses on an Electron binary: {name: "0"|"1"}. Returns what was
    changed. Fuses the binary does not have, or has removed, are left."""
    changed = {}
    with open(path, "r+b") as handle:
        data = mmap.mmap(handle.fileno(), 0)
        try:
            at = data.find(FUSE_SENTINEL)
            if at == -1:
                return changed
            start = at + len(FUSE_SENTINEL)
            length = data[start + 1]
            for index in range(length):
                name = FUSE_NAMES[index] if index < len(FUSE_NAMES) else f"fuse{index}"
                state = chr(data[start + 2 + index])
                if name in wanted and wanted[name] in "01" and state in "01" and state != wanted[name]:
                    data[start + 2 + index] = ord(wanted[name])
                    changed[name] = f"{state}->{wanted[name]}"
            data.flush()
        finally:
            data.close()
    return changed


# ----------------------------------------------------------------------- pck

PCK_MAGIC = b"GDPC"


def pck_locate(path):
    """(start, size, how) of a Godot pack: the whole file, a PE section named
    "pck", or the tail of an executable. None when there is none."""
    try:
        size = os.path.getsize(path)
        if size < 100:
            return None
        if read_at(path, 0, 4) == PCK_MAGIC:
            return (0, size, "standalone")
        info = pe_info(path)
        if info:
            for section in info["sections"]:
                if section["name"] == "pck" and read_at(path, section["raw"], 4) == PCK_MAGIC:
                    return (section["raw"], section["size"], "pe-section")
        ends = [size]
        if info and info["signature"][0] and info["signature"][0] < size:
            ends.append(info["signature"][0])  # a signature appended after the pack
        for end in ends:
            if end < 16:
                continue
            tail = read_at(path, end - 12, 12)
            if tail[8:] == PCK_MAGIC:
                length = struct.unpack("<Q", tail[:8])[0]
                start = end - 12 - length
                if 0 <= start and read_at(path, start, 4) == PCK_MAGIC:
                    return (start, length, "appended")
    except (OSError, struct.error):
        return None
    return None


def pck_read(path, start=0, list_limit=200000):
    """Header and directory of a pack at `start`. Paths are listed only when
    the directory is not encrypted."""
    with open(path, "rb") as handle:
        handle.seek(start)
        head = handle.read(4 + 16)
        if head[:4] != PCK_MAGIC:
            return None
        fmt, major, minor, patch = struct.unpack_from("<4I", head, 4)
        out = {
            "format": fmt,
            "engine": f"{major}.{minor}.{patch}",
            "major": major,
            "minor": minor,
            "patch": patch,
            "flags": 0,
            "file_base": 0,
            "files": None,
            "encrypted": False,
        }
        if fmt >= 2:
            out["flags"], out["file_base"] = struct.unpack("<IQ", handle.read(12))
            out["flags_at"] = start + 20
            out["file_base_at"] = start + 24
        out["encrypted"] = bool(out["flags"] & 1)
        if fmt >= 3:
            dir_offset = struct.unpack("<Q", handle.read(8))[0]
            handle.seek(start + dir_offset)
        else:
            handle.read(16 * 4)
        if out["encrypted"] or fmt > 3:
            return out
        raw = handle.read(4)
        if len(raw) < 4:
            return out
        count = struct.unpack("<I", raw)[0]
        out["file_count"] = count
        files = []
        offsets = []  # file position of each entry's offset field
        try:
            for _ in range(min(count, list_limit)):
                length = struct.unpack("<I", handle.read(4))[0]
                if length > 4096:
                    raise ValueError("path too long")
                name = handle.read(length).rstrip(b"\0").decode("utf-8", "replace")
                offsets.append(handle.tell())
                offset, size = struct.unpack("<QQ", handle.read(16))
                handle.read(16)
                flags = struct.unpack("<I", handle.read(4))[0] if fmt >= 2 else 0
                files.append({"path": name, "offset": offset, "size": size, "flags": flags})
        except (struct.error, ValueError):
            out["directory_error"] = True
        out["files"] = files
        out["offset_fields"] = offsets
    return out


def pck_extract(path, dest, located=None):
    """Write the pack of `path` to `dest` as a standalone .pck. Godot 3
    stores absolute file offsets in an embedded pack, Godot 4.0 to 4.2 an
    absolute file base: both are rewritten relative to the new file."""
    located = located or pck_locate(path)
    if not located:
        raise ValueError("no pack in this file")
    start, size, _ = located
    info = pck_read(path, start)
    with open(path, "rb") as source, open(dest, "wb") as out:
        source.seek(start)
        remaining = size
        while remaining > 0:
            chunk = source.read(min(remaining, 4 << 20))
            if not chunk:
                break
            out.write(chunk)
            remaining -= len(chunk)
    fixed = "none needed"
    if start:
        with open(dest, "r+b") as out:
            if info["format"] == 1 and info.get("files") is not None:
                for entry, at in zip(info["files"], info["offset_fields"]):
                    out.seek(at - start)
                    out.write(struct.pack("<Q", entry["offset"] - start))
                fixed = f"{len(info['files'])} absolute offsets made relative"
            elif info["format"] == 2 and not info["flags"] & 2:
                out.seek(info["file_base_at"] - start)
                out.write(struct.pack("<Q", info["file_base"] - start))
                fixed = "absolute file base made relative"
    return {"info": info, "fixed": fixed}


# ---------------------------------------------------------------- PyInstaller

PYI_MAGIC = b"MEI\x0c\x0b\x0a\x0b\x0e"


def pyinstaller_read(path):
    """The CArchive of a PyInstaller program: Python version, table of
    contents. None when there is none."""
    try:
        size = os.path.getsize(path)
        with open(path, "rb") as handle, mmap.mmap(handle.fileno(), 0, access=mmap.ACCESS_READ) as view:
            at = view.rfind(PYI_MAGIC)
            if at == -1 or at + 88 > size:
                return None
            _, length, toc_offset, toc_length, pyvers, pylib = struct.unpack_from("!8sIIII64s", view, at)
            start = at + 88 - length
            if start < 0:
                return None
            toc = view[start + toc_offset : start + toc_offset + toc_length]
            entries = []
            position = 0
            while position + 18 <= len(toc):
                entry_size, offset, csize, usize, cflag, typecode = struct.unpack_from("!IIIIBc", toc, position)
                if entry_size < 18 or position + entry_size > len(toc):
                    break
                name = toc[position + 18 : position + entry_size].split(b"\0", 1)[0].decode("utf-8", "replace")
                entries.append({"name": name, "offset": start + offset, "csize": csize, "usize": usize, "compressed": bool(cflag), "type": typecode.decode("latin-1")})
                position += entry_size
    except (OSError, ValueError, struct.error):
        return None
    return {
        "python": f"{pyvers // 100}.{pyvers % 100}",
        "pylib": pylib.rstrip(b"\0").decode("latin-1"),
        "start": start,
        "entries": entries,
    }


def pyinstaller_entry(path, entry):
    raw = read_at(path, entry["offset"], entry["csize"])
    return zlib.decompress(raw) if entry["compressed"] else raw


def pyz_names(blob):
    """Module names in a PYZ archive, without unmarshalling any code (the
    table is marshal data of plain tuples and strings, which is safe to load
    and independent of the Python version for these types)."""
    if blob[:4] != b"PYZ\0":
        return None
    toc_offset = struct.unpack("!I", blob[8:12])[0]
    try:
        toc = marshal.loads(blob[toc_offset:])
    except (ValueError, EOFError, TypeError):
        return None
    items = toc.items() if isinstance(toc, dict) else toc
    out = {}
    for name, value in items:
        if isinstance(name, bytes):
            name = name.decode("utf-8", "replace")
        out[name] = value  # (ispkg, position, length)
    return out


# ---------------------------------------------------------- .NET single file

DOTNET_BUNDLE_SIGNATURE = bytes.fromhex("8b1202b96a612038727b930214d7a03213f5b9e6efae3318ee3b2dce24b36aae")


def _read_7bit_string(view, position):
    length = 0
    shift = 0
    while True:
        byte = view[position]
        position += 1
        length |= (byte & 0x7F) << shift
        if not byte & 0x80:
            break
        shift += 7
    return view[position : position + length].decode("utf-8", "replace"), position + length


def dotnet_bundle_read(path):
    """The manifest of a .NET single-file bundle, or None."""
    try:
        with open(path, "rb") as handle, mmap.mmap(handle.fileno(), 0, access=mmap.ACCESS_READ) as view:
            at = view.find(DOTNET_BUNDLE_SIGNATURE)
            if at < 8:
                return None
            header_offset = struct.unpack_from("<q", view, at - 8)[0]
            if header_offset <= 0 or header_offset >= len(view):
                return None
            major, minor, count = struct.unpack_from("<IIi", view, header_offset)
            if major > 20 or count < 0 or count > 100000:
                return None
            bundle_id, position = _read_7bit_string(view, header_offset + 12)
            if major >= 2:
                position += 8 * 5
            files = []
            for _ in range(count):
                offset, size = struct.unpack_from("<qq", view, position)
                position += 16
                compressed = 0
                if major >= 6:
                    compressed = struct.unpack_from("<q", view, position)[0]
                    position += 8
                kind = view[position]
                position += 1
                name, position = _read_7bit_string(view, position)
                files.append({"path": name.replace("\\", "/"), "offset": offset, "size": size, "compressed": compressed, "type": kind})
    except (OSError, ValueError, struct.error, IndexError):
        return None
    return {"version": f"{major}.{minor}", "id": bundle_id, "files": files}


def dotnet_bundle_extract(path, bundle, dest):
    for entry in bundle["files"]:
        target = os.path.normpath(os.path.join(dest, entry["path"]))
        if not target.startswith(os.path.normpath(dest) + os.sep):
            continue
        os.makedirs(os.path.dirname(target), exist_ok=True)
        if entry["compressed"]:
            data = zlib.decompress(read_at(path, entry["offset"], entry["compressed"]), -15)
        else:
            data = read_at(path, entry["offset"], entry["size"])
        with open(target, "wb") as out:
            out.write(data)


# ----------------------------------------------------------------------- pbzx


def pbzx_decode(source, dest):
    """Apple's pbzx (a chunked xz stream around a cpio archive)."""
    with open(source, "rb") as handle, open(dest, "wb") as out:
        if handle.read(4) != b"pbzx":
            raise ValueError("not pbzx")
        flags = struct.unpack(">Q", handle.read(8))[0]
        while flags & (1 << 24):
            head = handle.read(16)
            if len(head) < 16:
                break
            flags, length = struct.unpack(">QQ", head)
            chunk = handle.read(length)
            if chunk[:6] == b"\xfd7zXZ\0":
                out.write(lzma.decompress(chunk))
            else:
                out.write(chunk)


def java_class_major(data):
    """Class-file major version (52 = Java 8, 61 = 17, 65 = 21...)."""
    if len(data) >= 8 and data[:4] == b"\xca\xfe\xba\xbe":
        return struct.unpack(">H", data[6:8])[0]
    return None


def text_or_none(raw):
    try:
        return raw.decode("utf-8-sig")
    except (UnicodeDecodeError, AttributeError):
        try:
            return raw.decode("latin-1")
        except (UnicodeDecodeError, AttributeError):
            return None


def bytes_io(data):
    return io.BytesIO(data)
