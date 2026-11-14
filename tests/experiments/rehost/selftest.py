#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Self-test of the detector's format readers on files made here.

    selftest.py SCRATCH_DIR

Builds small synthetic packages (an asar archive, a Windows program with a
fuse wire, Godot packs of format 1 and 2 standing alone and embedded, a
fused LÖVE game, a PyInstaller archive, a .NET single-file bundle, .NET
directories, a Ren'Py tree) and checks that each is read and classified as
intended. It runs before the corpus in CI; it executes none of the files it
makes. Exit status 1 when a check fails.
"""

import io
import json
import marshal
import os
import struct
import sys
import zipfile
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

from rehostlib import binfmt, detect, rehost  # noqa: E402

FAILED = []


def check(what, ok, detail=""):
    print(("  ok    " if ok else "  FAIL  ") + what + (f" -- {detail}" if detail and not ok else ""), flush=True)
    if not ok:
        FAILED.append(what)


def fake_pe(path, body=b"", clr=False, machine=0x8664, pad=8192):
    """A file with a valid PE header for x86-64, one section, then `body`
    as overlay."""
    optional = bytearray(240)
    struct.pack_into("<H", optional, 0, 0x20B)
    struct.pack_into("<H", optional, 68, 2)
    if clr:
        struct.pack_into("<II", optional, 112 + 8 * 14, 0x2000, 72)
    header = b"PE\0\0" + struct.pack("<HHIIIHH", machine, 1, 0, 0, 0, len(optional), 0x22)
    section = b".text\0\0\0" + struct.pack("<IIIIIIHHI", 4096, 0x1000, 4096, 1024, 0, 0, 0, 0, 0x60000020)
    data = bytearray(b"MZ" + b"\0" * 0x3A + struct.pack("<I", 0x80) + b"\0" * 0x40)
    data += header + optional + section
    data += b"\0" * (1024 - len(data))
    data += b"\x90" * 4096
    with open(path, "wb") as handle:
        handle.write(bytes(data) + body)


def make_pck(fmt, files, major=4, minor=3, patch=0, flags=0, absolute_base=0):
    """A Godot pack with a directory in the given format (1 or 2)."""
    entries = b""
    directory_size = 4
    for name in files:
        padded = len(name.encode()) + (4 - len(name.encode()) % 4) % 4
        directory_size += 4 + padded + 16 + 16 + (4 if fmt >= 2 else 0)
    header_size = 4 + 16 + (12 if fmt >= 2 else 0) + 64
    data_start = header_size + directory_size
    blob = b""
    for name, content in files.items():
        encoded = name.encode()
        padded = encoded + b"\0" * ((4 - len(encoded) % 4) % 4)
        offset = len(blob) if fmt >= 2 else absolute_base + data_start + len(blob)
        entries += struct.pack("<I", len(padded)) + padded + struct.pack("<QQ", offset, len(content)) + b"\0" * 16
        if fmt >= 2:
            entries += struct.pack("<I", 0)
        blob += content
    head = binfmt.PCK_MAGIC + struct.pack("<4I", fmt, major, minor, patch)
    if fmt >= 2:
        head += struct.pack("<IQ", flags, data_start if flags & 2 else absolute_base + data_start)
    head += b"\0" * 64
    return head + struct.pack("<I", len(files)) + entries + blob


def main():
    scratch = os.path.abspath(sys.argv[1])
    os.makedirs(scratch, exist_ok=True)

    print("asar")
    asar = os.path.join(scratch, "a.asar")
    binfmt.write_asar(asar, {"package.json": json.dumps({"name": "demo", "version": "1.2.3", "main": "main.js"}).encode(), "main.js": b"console.log(1)\n", "node_modules/x/package.json": b'{"name":"x","version":"2.0.0"}', "node_modules/x/build/Release/x.node": b"MZ" + b"\0" * 600})
    archive = binfmt.Asar(asar)
    check("header read", archive.read_json("package.json")["version"] == "1.2.3")
    check("nested file read", archive.read("main.js") == b"console.log(1)\n")
    archive.put_unpacked("node_modules/x/build/Release/x.node", b"\x7fELF" + b"\0" * 600, executable=True)
    archive.put_packed("package.json", b'{"name":"demo","version":"1.2.3","main":"shim.cjs"}')
    archive.put_packed("shim.cjs", b"// shim")
    archive.save()
    again = binfmt.Asar(asar)
    check("replaced file read after save", again.read_json("package.json")["main"] == "shim.cjs")
    check("untouched file still reads after save", again.read("main.js") == b"console.log(1)\n")
    check("added file reads", again.read("shim.cjs") == b"// shim")
    check("unpacked file reads from the .unpacked directory", again.read("node_modules/x/build/Release/x.node")[:4] == b"\x7fELF")

    print("PE, fuses, Electron layout")
    app = os.path.join(scratch, "electron-win")
    os.makedirs(os.path.join(app, "resources"))
    wire = binfmt.FUSE_SENTINEL + bytes([1, 8]) + b"01001101"
    fake_pe(os.path.join(app, "Demo.exe"), b"xx Chrome/128.0.6613.84 Electron/32.1.0 yy" + wire + b"\0" * 5000)
    binfmt.write_asar(os.path.join(app, "resources", "app.asar"), {"package.json": b'{"name":"demo","productName":"Demo","version":"1.2.3","main":"main.js"}', "main.js": b"1", "node_modules/k/package.json": b'{"name":"k","version":"7.9.0"}', "node_modules/k/build/Release/k.node": open(os.path.join(app, "Demo.exe"), "rb").read()[:2048]})
    info = binfmt.pe_info(os.path.join(app, "Demo.exe"))
    check("PE header parsed", info and info["machine"] == "x86_64" and info["overlay"] == 1024 + 4096, info)
    fuses = binfmt.read_fuses(os.path.join(app, "Demo.exe"))
    check("fuses read", fuses and fuses["RunAsNode"] == "0" and fuses["OnlyLoadAppFromAsar"] == "1", fuses)
    changed = binfmt.write_fuses(os.path.join(app, "Demo.exe"), {"RunAsNode": "1", "OnlyLoadAppFromAsar": "0"})
    check("fuses written", binfmt.read_fuses(os.path.join(app, "Demo.exe"))["RunAsNode"] == "1" and len(changed) == 2, changed)
    findings = detect.detect(app)
    first = findings[0] if findings else {}
    check("Electron detected with its version", first.get("runtime") == "electron" and first.get("version") == "32.1.0" and first.get("platform") == "windows", detect.summary(first))
    check("native module found with its package", [(n["package"], n["version"], n["format"]) for n in first.get("natives", [])] == [("k", "7.9.0", "pe")], first.get("natives"))
    check("verdict says native work is needed", first.get("verdict") == "rehostable-with-work" and "native-modules" in first.get("reasons", []))

    print("Godot packs")
    files = {"res://project.binary": b"ECFG" + b"\0" * 40, "res://main.gd": b"extends Node\n" * 10}
    for fmt, major in ((1, 3), (2, 4)):
        standalone = os.path.join(scratch, f"game{fmt}.pck")
        with open(standalone, "wb") as handle:
            handle.write(make_pck(fmt, files, major=major, minor=5, patch=1, flags=0))
        info = binfmt.pck_read(standalone)
        check(f"format {fmt}: standalone pack read", info and info["engine"] == f"{major}.5.1" and [f["path"] for f in info["files"]] == list(files), info and info.get("files"))
        carrier = os.path.join(scratch, f"game{fmt}.exe")
        fake_pe(carrier, b"")
        base = os.path.getsize(carrier)
        pack = make_pck(fmt, files, major=major, minor=5, patch=1, flags=0, absolute_base=base)
        with open(carrier, "ab") as handle:
            handle.write(pack + struct.pack("<Q", len(pack)) + binfmt.PCK_MAGIC)
        located = binfmt.pck_locate(carrier)
        check(f"format {fmt}: embedded pack located", located is not None and located[0] == base and located[2] == "appended", located)
        out = os.path.join(scratch, f"extracted{fmt}.pck")
        result = binfmt.pck_extract(carrier, out, located)
        info = binfmt.pck_read(out)
        entry = info["files"][1]
        start = entry["offset"] + (info["file_base"] if fmt >= 2 else 0)
        check(f"format {fmt}: extracted pack's offsets point at the data", binfmt.read_at(out, start, 12) == b"extends Node", (result["fixed"], start))
        tree = os.path.join(scratch, f"godot{fmt}")
        os.makedirs(tree, exist_ok=True)
        os.replace(carrier, os.path.join(tree, "Game.exe"))
        found = detect.detect(tree)
        check(f"format {fmt}: Godot detected from the program", bool(found) and found[0]["runtime"] == "godot" and found[0]["version"] == f"{major}.5.1", detect.summary(found[0] if found else None))

    print("LÖVE, launch4j")
    love = os.path.join(scratch, "love")
    os.makedirs(love, exist_ok=True)
    game = io.BytesIO()
    with zipfile.ZipFile(game, "w") as bundle:
        bundle.writestr("main.lua", "function love.draw() end\n")
        bundle.writestr("conf.lua", 'function love.conf(t)\n t.version = "11.4"\nend\n')
    fake_pe(os.path.join(love, "game.exe"), b"")
    with open(os.path.join(love, "game.exe"), "ab") as handle:
        handle.write(game.getvalue())
    found = detect.detect(love)
    check("fused LÖVE game detected", bool(found) and found[0]["runtime"] == "love" and found[0]["payload"]["conf_version"] == "11.4" and found[0]["payload"]["fused"], detect.summary(found[0] if found else None))
    java = os.path.join(scratch, "l4j")
    os.makedirs(java, exist_ok=True)
    jar = io.BytesIO()
    with zipfile.ZipFile(jar, "w") as bundle:
        bundle.writestr("META-INF/MANIFEST.MF", "Manifest-Version: 1.0\r\nMain-Class: demo.Main\r\n")
        bundle.writestr("demo/Main.class", b"\xca\xfe\xba\xbe\0\0\0\x3d" + b"\0" * 20)
    fake_pe(os.path.join(java, "tool.exe"), b"")
    with open(os.path.join(java, "tool.exe"), "ab") as handle:
        handle.write(b"launch4j" + jar.getvalue())
    found = detect.detect(java)
    check("jar in a Windows program detected, Java 17 classes", bool(found) and found[0]["runtime"] == "jvm" and found[0]["payload"]["needs_java"] == 17 and found[0]["payload"]["mainclass"] == "demo.Main", found[0]["payload"] if found else None)

    print("PyInstaller")
    frozen = os.path.join(scratch, "pyi")
    os.makedirs(frozen, exist_ok=True)
    code = zlib.compress(marshal.dumps(compile("X = 1", "m", "exec")))
    table = marshal.dumps([("mod", (0, 12, len(code)))])
    pyz = b"PYZ\0" + b"\0\0\0\0" + struct.pack("!I", 12 + len(code)) + code + table
    members = [(b"z", "PYZ-00.pyz", pyz), (b"s", "main", marshal.dumps(compile("print(1)", "main", "exec"))), (b"b", "_ssl.pyd", b"MZ"), (b"b", "Cryptodome\\Cipher\\_raw_aes.pyd", b"MZ")]
    body = b""
    toc = b""
    for typecode, name, data in members:
        packed = zlib.compress(data)
        encoded = name.encode() + b"\0"
        size = 18 + len(encoded) + (16 - (18 + len(encoded)) % 16) % 16
        toc += struct.pack("!IIIIBc", size, len(body), len(packed), len(data), 1, typecode) + encoded + b"\0" * (size - 18 - len(encoded))
        body += packed
    cookie = struct.pack("!8sIIII64s", binfmt.PYI_MAGIC, len(body) + len(toc) + 88, len(body), len(toc), 312, b"python312.dll")
    fake_pe(os.path.join(frozen, "tool.exe"), b"")
    with open(os.path.join(frozen, "tool.exe"), "ab") as handle:
        handle.write(body + toc + cookie)
    found = detect.detect(frozen)
    payload = found[0]["payload"] if found else {}
    check("PyInstaller program detected, Python 3.12", bool(found) and found[0]["runtime"] == "pyinstaller" and found[0]["version"] == "3.12" and payload["scripts"] == ["main"] and payload["extension_count"] == 2, payload)
    archive = binfmt.pyinstaller_read(os.path.join(frozen, "tool.exe"))
    names = binfmt.pyz_names(binfmt.pyinstaller_entry(os.path.join(frozen, "tool.exe"), archive["entries"][0]))
    check("PYZ table read", names is not None and "mod" in names, names)

    print(".NET")
    bundle_dir = os.path.join(scratch, "net-single")
    os.makedirs(bundle_dir, exist_ok=True)
    fake_pe(os.path.join(bundle_dir, "App.exe"), b"")
    base = os.path.getsize(os.path.join(bundle_dir, "App.exe"))
    config = json.dumps({"runtimeOptions": {"tfm": "net8.0", "includedFrameworks": [{"name": "Microsoft.NETCore.App", "version": "8.0.11"}]}}).encode()
    parts = [("App.runtimeconfig.json", config), ("App.dll", b"MZ" + b"\0" * 100), ("Avalonia.dll", b"MZ" + b"\0" * 50), ("System.Private.CoreLib.dll", b"MZ")]
    content = b""
    entries = b""
    for name, data in parts:
        entries += struct.pack("<qqq", base + len(content), len(data), 0) + b"\x01" + bytes([len(name)]) + name.encode()
        content += data
    header_at = base + len(content)
    header = struct.pack("<IIi", 6, 0, len(parts)) + bytes([12]) + b"abcdefghijkl" + b"\0" * 40 + entries
    with open(os.path.join(bundle_dir, "App.exe"), "ab") as handle:
        handle.write(content + header + struct.pack("<q", header_at) + binfmt.DOTNET_BUNDLE_SIGNATURE)
    found = detect.detect(bundle_dir)
    check(".NET single-file bundle detected", bool(found) and found[0]["runtime"] == "dotnet" and found[0]["payload"]["files"] == 4 and found[0]["payload"]["avalonia"], found[0]["payload"] if found else None)
    extracted = os.path.join(scratch, "net-single-out")
    binfmt.dotnet_bundle_extract(os.path.join(bundle_dir, "App.exe"), binfmt.dotnet_bundle_read(os.path.join(bundle_dir, "App.exe")), extracted)
    check("bundle extracted", open(os.path.join(extracted, "App.runtimeconfig.json"), "rb").read() == config)
    for name, frameworks, expect in (("net-wpf", [{"name": "Microsoft.NETCore.App", "version": "8.0.0"}, {"name": "Microsoft.WindowsDesktop.App", "version": "8.0.0"}], ("not-rehostable", "wine")), ("net-console", [{"name": "Microsoft.NETCore.App", "version": "9.0.0"}], ("rehostable", "rehost"))):
        directory = os.path.join(scratch, name)
        os.makedirs(directory, exist_ok=True)
        fake_pe(os.path.join(directory, "Tool.exe"), b"\0" * 5000)
        fake_pe(os.path.join(directory, "Tool.dll"), b"\0" * 5000, clr=True)
        with open(os.path.join(directory, "Tool.runtimeconfig.json"), "w") as handle:
            json.dump({"runtimeOptions": {"tfm": "net8.0", "frameworks": frameworks}}, handle)
        found = detect.detect(directory)
        check(f"{name}: {expect[0]}, route {expect[1]}", bool(found) and (found[0]["verdict"], found[0]["route"]) == expect and found[0]["payload"]["assembly"] == "Tool.dll", detect.summary(found[0] if found else None))

    print("Ren'Py, native")
    renpy = os.path.join(scratch, "vn", "Game-1.0-win")
    for sub in ("renpy", "game", "lib/py3-windows-x86_64", "lib/python3.9"):
        os.makedirs(os.path.join(renpy, sub), exist_ok=True)
    with open(os.path.join(renpy, "renpy", "vc_version.py"), "w") as handle:
        handle.write("branch = 'fix'\nnightly = False\nofficial = True\nversion = '8.3.4.24120703'\nversion_name = 'x'\n")
    open(os.path.join(renpy, "game", "script.rpyc"), "wb").write(b"RENPY RPC2")
    found = detect.detect(os.path.join(scratch, "vn"))
    check("Ren'Py detected with its version", bool(found) and found[0]["runtime"] == "renpy" and found[0]["version"] == "8.3.4.24120703" and found[0]["platform"] == "windows" and not found[0]["payload"]["carries_linux_runtime"], detect.summary(found[0] if found else None))
    qt = os.path.join(scratch, "qt")
    os.makedirs(qt, exist_ok=True)
    fake_pe(os.path.join(qt, "tool.exe"), b"\0" * 5000)
    fake_pe(os.path.join(qt, "Qt6Core.dll"), b"\0" * 5000)
    found = detect.detect(qt)
    check("Qt program: native, not re-hostable", bool(found) and found[0]["runtime"] == "native:qt" and found[0]["verdict"] == "not-rehostable" and found[0]["route"] == "wine", detect.summary(found[0] if found else None))
    check("the re-hoster refuses it", not rehost.rehost(found[0], os.path.join(scratch, "qt-out"), os.path.join(scratch, "cache"))["ok"] if found else False)

    print(f"\n{len(FAILED)} failed" if FAILED else "\nall passed")
    return 1 if FAILED else 0


if __name__ == "__main__":
    sys.exit(main())
