#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Checks TUR's Darling build before it is archived.

Run by build-darling.sh inside the build, after what TUR does not ship has
been removed. It refuses the tree (exit status 1) when:

  * a Mach-O file is signed with an Apple certificate: that is an Apple
    binary, which TUR never ships (AGENTS.md); Darling's own builds are
    ad-hoc signed;
  * a header, SDK, Swift interface or the Swift runtime directory is left;
  * a Mach-O file links the Swift runtime (/usr/lib/swift/...) without the
    weak flag: it could not load, because the runtime was removed;
  * an ELF file needs a host library that the base image does not have: the
    build would not start on TUR (unless --allow-missing-host-libraries).

It writes a JSON record of what it found either way. The Mach-O layout is
read from the published format description; nothing of Apple's is used.
"""

import argparse
import json
import os
import struct
import subprocess
import sys

APPLE_SIGNERS = (
    b"Apple Code Signing Certification Authority",
    b"Apple Root CA",
    b"Developer ID Certification Authority",
    b"Apple Worldwide Developer Relations",
)
FORBIDDEN_SUFFIXES = (".h", ".hpp", ".modulemap", ".swiftinterface", ".swiftmodule", ".tbd")
LC_REQ_DYLD = 0x80000000
LOAD_COMMANDS = {0xC: "load", 0x18 | LC_REQ_DYLD: "weak", 0x1F | LC_REQ_DYLD: "reexport",
                 0x20: "lazy", 0x23 | LC_REQ_DYLD: "upward"}
LC_CODE_SIGNATURE = 0x1D
MAX_COMMAND_BYTES = 4 * 1024 * 1024


def macho_slices(data):
    """(offset, size) of each thin image in a Mach-O file, or [] when it is not one."""
    if len(data) < 8:
        return []
    magic_be = struct.unpack_from(">I", data, 0)[0]
    if magic_be in (0xCAFEBABE, 0xCAFEBABF):
        count = struct.unpack_from(">I", data, 4)[0]
        if count == 0 or count > 44:
            return []  # a Java class file, or not a universal file
        wide = magic_be == 0xCAFEBABF
        slices = []
        for index in range(count):
            at = 8 + index * (32 if wide else 20)
            if wide:
                offset, size = struct.unpack_from(">QQ", data, at + 8)
            else:
                offset, size = struct.unpack_from(">II", data, at + 8)
            if offset + size <= len(data):
                slices.append((offset, size))
        return slices
    if magic_be in (0xFEEDFACE, 0xFEEDFACF, 0xCEFAEDFE, 0xCFFAEDFE):
        return [(0, len(data))]
    return []


def slice_facts(data, offset, size):
    """Linked libraries and the code signature of one thin image."""
    magic_le = struct.unpack_from("<I", data, offset)[0]
    if magic_le in (0xFEEDFACE, 0xFEEDFACF):
        order = "<"
    elif magic_le in (0xCEFAEDFE, 0xCFFAEDFE):
        order = ">"
    else:
        return [], b""
    wide = struct.unpack_from(order + "I", data, offset)[0] == 0xFEEDFACF
    header = 32 if wide else 28
    ncmds, sizeofcmds = struct.unpack_from(order + "II", data, offset + 16)
    if sizeofcmds > MAX_COMMAND_BYTES or header + sizeofcmds > size:
        return [], b""
    at = offset + header
    end = at + sizeofcmds
    libraries = []
    signature = b""
    for _ in range(ncmds):
        if at + 8 > end:
            break
        command, length = struct.unpack_from(order + "II", data, at)
        if length < 8 or at + length > end:
            break
        if command in LOAD_COMMANDS:
            name_at = struct.unpack_from(order + "I", data, at + 8)[0]
            raw = data[at + name_at:at + length].split(b"\0", 1)[0]
            libraries.append((LOAD_COMMANDS[command], raw.decode("utf-8", "replace")))
        elif command == LC_CODE_SIGNATURE:
            dataoff, datasize = struct.unpack_from(order + "II", data, at + 8)
            start = offset + dataoff
            signature = data[start:start + min(datasize, size)]
        at += length
    return libraries, signature


def elf_needed(path):
    output = subprocess.run(["readelf", "-d", "--wide", path], capture_output=True, text=True).stdout
    needed = []
    for line in output.splitlines():
        if "(NEEDED)" in line and "[" in line:
            needed.append(line.split("[", 1)[1].split("]", 1)[0])
    return needed


def system_libraries():
    """soname -> path, from ldconfig's cache, for 64-bit x86 libraries."""
    found = {}
    output = subprocess.run(["ldconfig", "-p"], capture_output=True, text=True).stdout
    for line in output.splitlines()[1:]:
        line = line.strip()
        if "=>" not in line or "x86-64" not in line:
            continue
        name = line.split(" ", 1)[0]
        found.setdefault(name, line.split("=>", 1)[1].strip())
    return found


def package_of(path):
    result = subprocess.run(["rpm", "-qf", "--qf", "%{NAME}\n", path], capture_output=True, text=True)
    return result.stdout.splitlines()[0] if result.returncode == 0 and result.stdout else None


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--tree", required=True)
    parser.add_argument("--base-packages", required=True)
    parser.add_argument("--record", required=True)
    parser.add_argument("--allow-missing-host-libraries", action="store_true")
    args = parser.parse_args()

    tree = os.path.abspath(args.tree)
    base = set(open(args.base_packages).read().split())
    violations = []
    macho_files = 0
    elf_files = 0
    needed = {}
    own_names = set()

    swift = os.path.join(tree, "libexec/darling/usr/lib/swift")
    if os.path.lexists(swift):
        violations.append({"path": swift[len(tree):], "why": "the Swift runtime directory is still there"})

    for root, dirs, files in os.walk(tree):
        for name in dirs:
            if name == "Developer" or name.endswith(".sdk"):
                violations.append({"path": os.path.join(root, name)[len(tree):], "why": "an SDK or developer directory"})
        for name in files:
            path = os.path.join(root, name)
            relative = path[len(tree):]
            own_names.add(name)
            if name.endswith(FORBIDDEN_SUFFIXES):
                violations.append({"path": relative, "why": "a header, interface or stub file"})
            if os.path.islink(path) or not os.path.isfile(path):
                continue
            with open(path, "rb") as handle:
                head = handle.read(4)
                if head == b"\x7fELF":
                    elf_files += 1
                    for soname in elf_needed(path):
                        needed.setdefault(soname, []).append(relative)
                    continue
                if len(head) < 4 or struct.unpack(">I", head)[0] not in (
                        0xCAFEBABE, 0xCAFEBABF, 0xFEEDFACE, 0xFEEDFACF, 0xCEFAEDFE, 0xCFFAEDFE):
                    continue
                handle.seek(0)
                data = handle.read()
            slices = macho_slices(data)
            if not slices:
                continue
            macho_files += 1
            for offset, size in slices:
                libraries, signature = slice_facts(data, offset, size)
                if any(signer in signature for signer in APPLE_SIGNERS):
                    violations.append({"path": relative, "why": "signed with an Apple certificate: an Apple binary"})
                for kind, library in libraries:
                    if library.startswith("/usr/lib/swift/") and kind != "weak":
                        violations.append({"path": relative, "why": "links the Swift runtime, which is not shipped: " + library})

    system = system_libraries()
    host_needs = []
    missing = []
    for soname in sorted(needed):
        if soname in own_names:
            continue
        path = system.get(soname)
        package = package_of(path) if path else None
        entry = {"soname": soname, "path": path, "package": package,
                 "in_base": bool(package and package in base), "needed_by": needed[soname][:5]}
        host_needs.append(entry)
        if not entry["in_base"]:
            missing.append(entry)

    record = {
        "macho_files": macho_files,
        "elf_files": elf_files,
        "violations": violations,
        "host_libraries": host_needs,
        "missing_host_libraries": missing,
    }
    with open(args.record, "w") as handle:
        json.dump(record, handle, indent=1)
    print(f"Mach-O files: {macho_files}, ELF files: {elf_files}, host libraries: {len(host_needs)}")
    for violation in violations[:50]:
        print(f"NOT SHIPPED: {violation['path']}: {violation['why']}")
    for entry in missing:
        print(f"HOST LIBRARY NOT IN THE BASE: {entry['soname']} (package {entry['package']}), "
              f"needed by {', '.join(entry['needed_by'])}")
    if violations:
        return 1
    if missing and not args.allow_missing_host_libraries:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
