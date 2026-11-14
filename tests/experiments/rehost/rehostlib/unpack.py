# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Opening a package without running it: archives, Windows installers and
macOS disk images, layer by layer, into a directory.

Tools used (all Fedora packages): bsdtar (libarchive), 7z (7-Zip),
innoextract, msiextract (msitools), unsquashfs. No installer is executed;
each tool reads the container format. What each layer was is recorded, so
the report can say which containers could be opened.

A spike: the size limits and hostile-input rules of tur-runtime's
inspection (docs/architecture/runtime-manager.md) are not implemented here.
"""

import os
import shutil
import struct
import subprocess

from . import binfmt


def sniff(path):
    """The container kind of a file, from its content."""
    try:
        size = os.path.getsize(path)
        head = binfmt.read_at(path, 0, 1024)
    except OSError:
        return "unreadable"
    if head[:4] == b"PK\x03\x04" or head[:4] == b"PK\x05\x06":
        return "zip"
    if head[:6] == b"7z\xbc\xaf\x27\x1c":
        return "7z"
    if head[:8] == b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1":
        return "ole"
    if head[:4] == b"xar!":
        return "xar"
    if head[:8] == b"!<arch>\n":
        return "ar"
    if head[:4] == binfmt.PCK_MAGIC:
        return "pck"
    if head[:2] == b"\x1f\x8b" or head[:3] == b"BZh" or head[:6] == b"\xfd7zXZ\0" or head[:4] == b"\x28\xb5\x2f\xfd":
        return "tar-compressed"
    if len(head) > 262 and head[257:262] == b"ustar":
        return "tar"
    if head[:2] == b"MZ":
        return "pe"
    if head[:4] == b"\x7fELF":
        if head[8:11] == b"AI\x02":
            return "appimage"
        return "elf"
    if size > 512 and binfmt.read_at(path, size - 512, 4) == b"koly":
        return "dmg"
    if binfmt.binary_kind(path):
        return "macho"
    return "other"


class Unpacker:
    def __init__(self, log=print, timeout=900):
        self.log = log
        self.timeout = timeout
        self.layers = []  # what was peeled, outermost first
        self.problems = []

    def run(self, command, cwd=None):
        self.log("    $ " + " ".join(command))
        try:
            done = subprocess.run(command, cwd=cwd, capture_output=True, text=True, errors="replace", timeout=self.timeout)
        except subprocess.TimeoutExpired:
            self.problems.append(f"{command[0]} timed out")
            return 124, ""
        except FileNotFoundError:
            self.problems.append(f"{command[0]} is not installed")
            return 127, ""
        output = (done.stdout[-1500:] + done.stderr[-1500:]).strip()
        if done.returncode != 0:
            self.log("      exit " + str(done.returncode) + ": " + output[-600:].replace("\n", " | "))
        return done.returncode, output

    # ---- individual containers

    def bsdtar(self, path, dest):
        os.makedirs(dest, exist_ok=True)
        code, _ = self.run(["bsdtar", "-xf", path, "-C", dest, "--no-same-owner", "--no-same-permissions"])
        return code == 0 or bool(os.listdir(dest))

    def seven(self, path, dest, include=None):
        os.makedirs(dest, exist_ok=True)
        command = ["7z", "x", "-y", "-bso0", "-bsp0", f"-o{dest}", path]
        if include:
            command += include
        code, output = self.run(command)
        if code not in (0, 1) and not os.listdir(dest):
            self.problems.append(f"7z could not open {os.path.basename(path)}: {output[-200:]}")
            return False
        return bool(os.listdir(dest))

    def appimage(self, path, dest):
        head = binfmt.read_at(path, 0, 64)
        shoff = struct.unpack_from("<Q", head, 0x28)[0]
        shentsize, shnum = struct.unpack_from("<HH", head, 0x3A)
        offset = shoff + shentsize * shnum
        code, _ = self.run(["unsquashfs", "-q", "-n", "-f", "-o", str(offset), "-d", dest, path])
        return code == 0 or (os.path.isdir(dest) and bool(os.listdir(dest)))

    def deb(self, path, dest):
        holder = dest + ".ar"
        if not self.bsdtar(path, holder):
            return False
        data = next((os.path.join(holder, name) for name in sorted(os.listdir(holder)) if name.startswith("data.tar")), None)
        ok = bool(data) and self.bsdtar(data, dest)
        shutil.rmtree(holder, ignore_errors=True)
        return ok

    def pkg(self, path, dest):
        holder = dest + ".xar"
        if not (self.bsdtar(path, holder) or self.seven(path, holder)):
            return False
        os.makedirs(dest, exist_ok=True)
        count = 0
        for root, _, files in os.walk(holder):
            for name in files:
                if name != "Payload":
                    continue
                payload = os.path.join(root, name)
                target = os.path.join(dest, os.path.relpath(root, holder).replace(os.sep, "_"))
                head = binfmt.read_at(payload, 0, 4)
                if head == b"pbzx":
                    plain = payload + ".cpio"
                    try:
                        binfmt.pbzx_decode(payload, plain)
                    except (ValueError, OSError, struct.error) as problem:
                        self.problems.append(f"pbzx payload: {problem}")
                        continue
                    self.layers.append("pbzx")
                    payload = plain
                if self.bsdtar(payload, target):
                    count += 1
        shutil.rmtree(holder, ignore_errors=True)
        return count > 0

    def dmg(self, path, dest):
        if not self.seven(path, dest):
            return False
        # 7-Zip may give the partitions as image files: open those too.
        for _ in range(3):
            images = [
                os.path.join(dest, name)
                for name in os.listdir(dest)
                if name.lower().endswith((".hfs", ".hfsx", ".apfs", ".img")) and os.path.isfile(os.path.join(dest, name))
            ]
            if not images:
                break
            for image in images:
                inner = image + ".d"
                if self.seven(image, inner):
                    self.layers.append(os.path.splitext(image)[1].lstrip("."))
                os.unlink(image)
        return bool(os.listdir(dest))

    # ---- Windows programs that are installers

    def pe(self, path, dest):
        """Returns the kind of installer, having extracted it to dest, or
        None when the program is not a container this code opens."""
        info = binfmt.pe_info(path)
        if not info:
            return None
        overlay = info["overlay"]
        head = binfmt.read_at(path, overlay, 1 << 16) if info["overlay_size"] else b""
        if b"\xef\xbe\xad\xdeNullsoftInst" in head[:4096] or b"NullsoftInst" in head[:1024]:
            if not self.seven(path, dest):
                return None
            self.layers.append("nsis")
            self.nested_app_archive(dest)
            return "nsis"
        if binfmt.contains(path, b"Inno Setup Setup Data") or binfmt.contains(path, b"rDlPtS"):
            os.makedirs(dest, exist_ok=True)
            code, output = self.run(["innoextract", "-s", "-d", dest, path])
            if code == 0 and os.listdir(dest):
                self.layers.append("inno-setup")
                return "inno-setup"
            self.problems.append("innoextract could not open this Inno Setup installer: " + output[-200:])
            return None
        if binfmt.pyinstaller_read(path) or binfmt.pck_locate(path) or binfmt.dotnet_bundle_read(path):
            return None  # the program is the payload's carrier: the detector reads it in place
        if head[:6] == b"7z\xbc\xaf\x27\x1c" or b"7z\xbc\xaf\x27\x1c" in head[:8192]:
            if self.seven(path, dest):
                self.layers.append("7z-sfx")
                self.nested_app_archive(dest)
                return "7z-sfx"
        tail = binfmt.zip_tail(path)
        if tail:
            # Squirrel.Windows' Setup.exe ends in a zip that holds the
            # application as a NuGet package; anything else that ends in a
            # zip (launch4j, LÖVE, a self-extracting zip) is read in place.
            try:
                import zipfile

                with zipfile.ZipFile(path) as archive:
                    packages = [name for name in archive.namelist() if name.endswith(".nupkg")]
            except (zipfile.BadZipFile, OSError):
                packages = []
            if not packages:
                return None
            holder = dest + ".zip"
            with open(path, "rb") as source, open(holder, "wb") as out:
                source.seek(tail["start"])
                shutil.copyfileobj(source, out)
            ok = self.bsdtar(holder, dest)
            os.unlink(holder)
            if not ok:
                return None
            self.layers.append("squirrel-setup")
            for name in packages:
                package = os.path.join(dest, name)
                if os.path.isfile(package):
                    if self.bsdtar(package, os.path.join(dest, "nupkg")):
                        self.layers.append("nupkg")
                    os.unlink(package)
            return "squirrel"
        # Squirrel.Windows keeps a zip in the program's resources; other
        # bootstrappers keep cabinets there. 7-Zip opens the resource tree.
        squirrel = binfmt.contains(path, "SquirrelTemp".encode("utf-16-le")) or binfmt.contains(path, b"Squirrel")
        if squirrel or info["overlay_size"] > (1 << 20):
            holder = dest + ".rsrc"
            if self.seven(path, holder):
                found = None
                for root, _, files in os.walk(holder):
                    for name in files:
                        inner = os.path.join(root, name)
                        if sniff(inner) == "zip" and os.path.getsize(inner) > (1 << 20):
                            found = inner
                if found and self.bsdtar(found, dest):
                    self.layers.append("squirrel-setup" if squirrel else "pe-resource-zip")
                    for name in sorted(os.listdir(dest)):
                        if name.endswith(".nupkg"):
                            package = os.path.join(dest, name)
                            if self.bsdtar(package, os.path.join(dest, "nupkg")):
                                self.layers.append("nupkg")
                            os.unlink(package)
                    shutil.rmtree(holder, ignore_errors=True)
                    return "squirrel" if squirrel else "pe-resource-zip"
            shutil.rmtree(holder, ignore_errors=True)
        return None

    def nested_app_archive(self, dest):
        """electron-builder's NSIS installers carry the application as
        $PLUGINSDIR/app-64.7z (and app-32.7z, app-arm64.7z)."""
        for root, _, files in os.walk(dest):
            archives = sorted(name for name in files if name.startswith("app-") and name.endswith(".7z"))
            if not archives:
                continue
            wanted = "app-64.7z" if "app-64.7z" in archives else archives[0]
            if self.seven(os.path.join(root, wanted), os.path.join(dest, "app")):
                self.layers.append(f"7z ({wanted})")
            for name in archives:
                os.unlink(os.path.join(root, name))
            return

    # ---- the whole thing

    def unpack(self, path, dest):
        """Open `path` into `dest`. Returns the kind of the outermost
        container. When the file is not a container (a program that carries
        its payload inside itself, or something unknown), it is copied
        into dest unchanged, so that the detector always gets a directory."""
        kind = sniff(path)
        self.log(f"  unpack: {os.path.basename(path)} is {kind}")
        ok = False
        if kind in ("zip", "tar", "tar-compressed"):
            ok = self.bsdtar(path, dest)
            if ok:
                self.layers.append(kind)
        elif kind == "7z":
            ok = self.seven(path, dest)
            if ok:
                self.layers.append("7z")
                self.nested_app_archive(dest)
        elif kind == "ole":
            os.makedirs(dest, exist_ok=True)
            code, _ = self.run(["msiextract", "-C", dest, path])
            ok = code == 0 and bool(os.listdir(dest))
            if not ok:
                ok = self.seven(path, dest)
            if ok:
                self.layers.append("msi")
        elif kind == "xar":
            ok = self.pkg(path, dest)
            if ok:
                self.layers.append("pkg")
        elif kind == "ar":
            ok = self.deb(path, dest)
            if ok:
                self.layers.append("deb")
        elif kind == "dmg":
            ok = self.dmg(path, dest)
            if ok:
                self.layers.append("dmg")
        elif kind == "appimage":
            ok = self.appimage(path, dest)
            if ok:
                self.layers.append("appimage")
        elif kind == "pe":
            ok = self.pe(path, dest) is not None
        if not ok:
            os.makedirs(dest, exist_ok=True)
            shutil.copy2(path, os.path.join(dest, os.path.basename(path)))
            if kind not in ("pe", "elf", "macho", "pck", "other"):
                self.problems.append(f"the {kind} container could not be opened")
            self.layers.append(f"file ({kind})")
        # A lone archive inside an archive (a .dmg in a zip, a tar in a gz).
        entries = [name for name in os.listdir(dest) if not name.startswith(("__MACOSX", "."))]
        if ok and len(entries) == 1:
            inner = os.path.join(dest, entries[0])
            if os.path.isfile(inner) and sniff(inner) in ("zip", "tar", "tar-compressed", "7z", "dmg", "xar", "ole"):
                moved = dest + ".inner"
                os.replace(inner, moved)
                shutil.rmtree(dest, ignore_errors=True)
                kind_inner = self.unpack(moved, dest)
                os.unlink(moved)
                return kind + "+" + kind_inner
        return kind
