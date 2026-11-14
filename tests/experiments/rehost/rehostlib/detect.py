# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Which runtime does an unpacked package carry, and at which version?

detect(root) walks a directory (what unpack.py produced) and returns
findings, most significant first. Nothing is executed: versions are read
from version strings in binaries, property lists, JSON files and archive
headers.

A finding says:
  runtime    electron | nwjs | dotnet | jvm | godot | renpy | love |
             pyinstaller | native:<toolkit>
  platform   windows | macos | linux    (what the package was built for)
  version    of the runtime, or None, with the evidence it was read from
  verdict    rehostable | rehostable-with-work | not-rehostable
  route      rehost | wine | native-only | unknown   (the router's word)
  reasons    machine-readable codes;  notes: sentences for a person
  payload    where the portable part is (runtime-specific)
  natives    platform-specific code that travels with the payload
"""

import json
import os
import plistlib
import re
import zipfile

from . import binfmt

SKIP_DIRS = {"node_modules", "locales", "__MACOSX", ".git", "swiftshader", "legal", "$PLUGINSDIR", "$TEMP"}
BINARY_EXT = (".exe", ".dll", ".node", ".dylib", ".so", ".pyd", ".jnilib", ".sys")


def kind_from_head(head):
    """("pe"|"elf"|"macho", [archs]) from the first bytes of a file."""
    import struct

    if len(head) < 20:
        return None
    if head[:2] == b"MZ":
        if len(head) < 0x40:
            return ("pe", ["?"])
        lfanew = struct.unpack_from("<I", head, 0x3C)[0]
        if lfanew + 6 <= len(head) and head[lfanew : lfanew + 4] == b"PE\0\0":
            machine = struct.unpack_from("<H", head, lfanew + 4)[0]
            return ("pe", [binfmt.PE_MACHINES.get(machine, hex(machine))])
        return ("pe", ["?"])
    if head[:4] == b"\x7fELF":
        order = "<" if head[5] == 1 else ">"
        machine = struct.unpack_from(order + "H", head, 18)[0]
        return ("elf", [binfmt.ELF_MACHINES.get(machine, str(machine))])
    magic = head[:4]
    if magic in (b"\xcf\xfa\xed\xfe", b"\xce\xfa\xed\xfe"):
        return ("macho", [binfmt.MACHO_CPUS.get(struct.unpack_from("<I", head, 4)[0], "?")])
    if magic in (b"\xfe\xed\xfa\xcf", b"\xfe\xed\xfa\xce"):
        return ("macho", [binfmt.MACHO_CPUS.get(struct.unpack_from(">I", head, 4)[0], "?")])
    if magic in (b"\xca\xfe\xba\xbe", b"\xca\xfe\xba\xbf"):
        count = struct.unpack_from(">I", head, 4)[0]
        if count > 44:
            return None
        step = 20 if magic == b"\xca\xfe\xba\xbe" else 32
        return ("macho", [binfmt.MACHO_CPUS.get(struct.unpack_from(">I", head, 8 + i * step)[0], "?") for i in range(count) if 12 + i * step <= len(head)])
    return None


PLATFORM_OF = {"pe": "windows", "macho": "macos", "elf": "linux"}


class Payload:
    """The application files of an Electron or NW.js program: an asar
    archive with its unpacked directory, or a plain directory. Paths are
    relative, with forward slashes."""

    def __init__(self, asar_path=None, directory=None):
        self.asar = binfmt.Asar(asar_path) if asar_path else None
        self.directory = directory
        self.kind = "asar" if asar_path else "directory"

    def read(self, rel, limit=8 << 20):
        if self.asar:
            return self.asar.read(rel, limit)
        try:
            with open(os.path.join(self.directory, rel), "rb") as handle:
                return handle.read(limit)
        except OSError:
            return None

    def read_json(self, rel):
        raw = self.read(rel, 4 << 20)
        if raw is None:
            return None
        try:
            return json.loads(raw.decode("utf-8-sig"))
        except (ValueError, UnicodeDecodeError):
            return None

    def exists(self, rel):
        if self.asar:
            return self.asar.node(rel) is not None
        return os.path.exists(os.path.join(self.directory, rel))

    def listdir(self, rel):
        if self.asar:
            node = self.asar.node(rel) if rel else self.asar.header
            return sorted(node.get("files", {})) if node and "files" in node else []
        try:
            return sorted(os.listdir(os.path.join(self.directory, rel)))
        except OSError:
            return []

    def files(self):
        """(relative path, size, where) of every file."""
        if self.asar:
            for rel, node, is_dir in self.asar.walk():
                if not is_dir and "link" not in node:
                    yield rel, int(node.get("size", 0)), "unpacked" if node.get("unpacked") else "packed"
        else:
            for root, dirs, names in os.walk(self.directory):
                for name in names:
                    path = os.path.join(root, name)
                    if os.path.islink(path):
                        continue
                    try:
                        yield os.path.relpath(path, self.directory).replace(os.sep, "/"), os.path.getsize(path), "directory"
                    except OSError:
                        continue

    def package_of(self, rel):
        """The nearest package.json above a file: (directory, name, version, json)."""
        parts = rel.split("/")[:-1]
        while parts:
            data = self.read_json("/".join(parts + ["package.json"]))
            if isinstance(data, dict) and data.get("name"):
                return "/".join(parts), data.get("name"), str(data.get("version") or ""), data
            parts.pop()
        return None, None, None, None

    def natives(self):
        """Every .node file, with the format of its machine code."""
        out = []
        for rel, size, where in self.files():
            if not rel.endswith(".node") or size < 512:
                continue
            kind = kind_from_head(self.read(rel, 4096) or b"")
            if not kind:
                continue
            directory, name, version, _ = self.package_of(rel)
            out.append({"path": rel, "where": where, "format": kind[0], "archs": kind[1], "size": size, "package": name, "version": version, "package_dir": directory})
        return out


def _plist(path):
    try:
        with open(path, "rb") as handle:
            data = plistlib.load(handle)
        return data if isinstance(data, dict) else {}
    except Exception:  # plistlib raises several unrelated types on bad input
        return {}


def _exes(directory, names, kinds=("pe",)):
    out = []
    for name in names:
        path = os.path.join(directory, name)
        if os.path.isfile(path) and not os.path.islink(path) and os.path.getsize(path) > 4096:
            kind = binfmt.binary_kind(path)
            if kind and kind[0] in kinds and not name.lower().endswith((".dll", ".so", ".dylib", ".node", ".pyd")) and ".so." not in name:
                out.append((os.path.getsize(path), name, kind))
    return sorted(out, reverse=True)


def sidecars(directory, skip=()):
    """Machine-code files below a directory that are not Node modules:
    helper programs and libraries built for one platform."""
    out = []
    for root, dirs, names in os.walk(directory):
        dirs[:] = [name for name in dirs if os.path.join(root, name) not in skip]
        for name in names:
            path = os.path.join(root, name)
            lower = name.lower()
            if os.path.islink(path) or lower.endswith(".node"):
                continue
            if not (lower.endswith(BINARY_EXT) or ".so." in lower or "." not in name):
                continue
            try:
                if os.path.getsize(path) < 4096:
                    continue
            except OSError:
                continue
            kind = kind_from_head(binfmt.read_at(path, 0, 4096))
            if kind:
                out.append({"path": os.path.relpath(path, directory).replace(os.sep, "/"), "format": kind[0], "archs": kind[1], "size": os.path.getsize(path)})
            if len(out) >= 400:
                return out
    return out


ELECTRON_UA = rb"Chrome/[0-9.]{6,20} Electron/([0-9]+\.[0-9]+\.[0-9]+(?:-[a-z]+\.[0-9]+)?(?:\+[a-z0-9]+)?)"
CHROME_VERSION = rb"Chrome/([0-9]{2,3}\.0\.[0-9]{4}\.[0-9]{1,3})"


def electron_payload(resources):
    asar = os.path.join(resources, "app.asar")
    if os.path.isfile(asar):
        try:
            return Payload(asar_path=asar)
        except (ValueError, OSError):
            return None
    for name in ("app", "default_app"):
        if os.path.isfile(os.path.join(resources, name, "package.json")):
            return Payload(directory=os.path.join(resources, name))
    return None


def native_payload(payload, resources):
    """A universal macOS build keeps one archive per processor
    (app-x64.asar, app-arm64.asar) behind a small app.asar that picks one:
    the native modules are in the per-processor archive."""
    if resources and payload.kind == "asar":
        split = os.path.join(resources, "app-x64.asar")
        if os.path.isfile(split):
            try:
                return Payload(asar_path=split)
            except (ValueError, OSError):
                pass
    return payload


def describe_web_payload(finding, payload, resources):
    """What an Electron or NW.js payload holds: its package.json, native
    modules, and the other platform-specific files beside it."""
    package = payload.read_json("package.json") or {}
    finding["payload"].update(
        {
            "kind": payload.kind,
            "name": package.get("productName") or package.get("name"),
            "package_name": package.get("name"),
            "app_version": package.get("version"),
            "main": package.get("main"),
            "module_type": package.get("type"),
        }
    )
    natives = native_payload(payload, resources).natives()
    finding["natives"] = natives
    foreign = [n for n in natives if n["format"] != "elf"]
    linux = [n for n in natives if n["format"] == "elf" and "x86_64" in n["archs"]]
    finding["payload"]["native_modules"] = {"total": len(natives), "foreign": len(foreign), "linux_x64": len(linux)}
    if resources:
        skip = (os.path.join(resources, "app"),) if payload.kind == "directory" else ()
        found = sidecars(resources, skip=skip)
        if payload.kind == "directory":
            found += [dict(item, path="app/" + item["path"]) for item in sidecars(payload.directory)]
        finding["sidecars"] = found
        finding["payload"]["sidecars"] = {
            "total": len(found),
            "foreign": sum(1 for item in found if item["format"] != "elf"),
        }
        finding["payload"]["updater"] = (
            "electron-updater" if os.path.exists(os.path.join(resources, "app-update.yml")) else None
        )
    if foreign:
        finding["reasons"].append("native-modules")
        finding["verdict"] = "rehostable-with-work"
        finding["notes"].append(f"{len(foreign)} native Node module(s) built for {PLATFORM_OF[foreign[0]['format']]} need a Linux build")
    if finding.get("sidecars") and any(item["format"] != "elf" for item in finding["sidecars"]):
        finding["reasons"].append("platform-helper-binaries")


def finding_base(runtime, platform, where):
    return {
        "runtime": runtime,
        "platform": platform,
        "app_dir": where,
        "version": None,
        "version_evidence": [],
        "verdict": "rehostable",
        "route": "rehost",
        "reasons": [],
        "notes": [],
        "payload": {},
        "natives": [],
        "sidecars": [],
    }


def detect_electron_dir(directory, names):
    """Windows and Linux layout: <app>/resources/app.asar beside the program."""
    resources = os.path.join(directory, "resources")
    payload = electron_payload(resources)
    if payload is None:
        return None
    exes = _exes(directory, names, kinds=("pe", "elf"))
    if not exes:
        return None
    lower = {name.lower() for name in names}
    if "nw.dll" in lower or "nw_elf.dll" in lower or "nw.pak" in lower or "nw_100_percent.pak" in lower:
        return None
    _, main, kind = exes[0]
    finding = finding_base("electron", PLATFORM_OF[kind[0]], directory)
    finding["arch"] = kind[1]
    finding["payload"] = {"resources": resources, "program": main}
    binary = os.path.join(directory, main)
    for value in binfmt.scan(binary, ELECTRON_UA, limit=3):
        finding["version_evidence"].append({"from": f"user-agent string in {main}", "value": value})
    version_file = os.path.join(directory, "version")
    if os.path.isfile(version_file):
        text = binfmt.text_or_none(binfmt.read_at(version_file, 0, 64)) or ""
        if re.fullmatch(r"v?\d+\.\d+\.\d+\S*", text.strip()):
            finding["version_evidence"].append({"from": "version file", "value": text.strip().lstrip("v")})
    chrome = binfmt.scan(binary, CHROME_VERSION, limit=3)
    finding["payload"]["chromium"] = chrome[0] if chrome else None
    finding["payload"]["fuses"] = binfmt.read_fuses(binary)
    if kind[0] == "pe":
        finding["payload"]["pe_version"] = binfmt.pe_version_strings(binary)
    if finding["version_evidence"]:
        finding["version"] = finding["version_evidence"][0]["value"]
    else:
        finding["reasons"].append("runtime-version-unknown")
        finding["verdict"] = "rehostable-with-work"
    if finding["version"] and "+" in finding["version"]:
        finding["reasons"].append("vendor-electron-build")
        finding["notes"].append("a vendor build of Electron (for example castlabs' with Widevine): the stock build lacks what the vendor added")
    describe_web_payload(finding, payload, resources)
    return finding


def detect_electron_app(bundle):
    """macOS layout: X.app/Contents/Frameworks/Electron Framework.framework."""
    framework = os.path.join(bundle, "Contents", "Frameworks", "Electron Framework.framework")
    if not os.path.isdir(framework):
        return None
    resources = os.path.join(bundle, "Contents", "Resources")
    payload = electron_payload(resources)
    if payload is None:
        return None
    finding = finding_base("electron", "macos", bundle)
    info = _plist(os.path.join(bundle, "Contents", "Info.plist"))
    finding["payload"] = {"resources": resources, "program": info.get("CFBundleExecutable"), "bundle_id": info.get("CFBundleIdentifier")}
    binary = None
    for candidate in ("Versions/A/Electron Framework", "Electron Framework"):
        path = os.path.join(framework, candidate)
        if os.path.isfile(path) and not os.path.islink(path):
            binary = path
            break
    if binary:
        kind = binfmt.binary_kind(binary)
        finding["arch"] = kind[1] if kind else []
        for value in binfmt.scan(binary, ELECTRON_UA, limit=3):
            finding["version_evidence"].append({"from": "user-agent string in Electron Framework", "value": value})
        chrome = binfmt.scan(binary, CHROME_VERSION, limit=3)
        finding["payload"]["chromium"] = chrome[0] if chrome else None
        finding["payload"]["fuses"] = binfmt.read_fuses(binary)
    for candidate in ("Versions/A/Resources/Info.plist", "Resources/Info.plist"):
        version = _plist(os.path.join(framework, candidate)).get("CFBundleVersion")
        if version:
            finding["version_evidence"].append({"from": "Electron Framework Info.plist", "value": str(version)})
            break
    if finding["version_evidence"]:
        finding["version"] = finding["version_evidence"][0]["value"]
    else:
        finding["reasons"].append("runtime-version-unknown")
        finding["verdict"] = "rehostable-with-work"
    describe_web_payload(finding, payload, resources)
    return finding


NW_HINTS = rb"(?:nwjs|nw\.js|node-webkit)[ /]v?([0-9]+\.[0-9]+\.[0-9]+)"


def nw_version_evidence(finding, binaries):
    for binary in binaries:
        if not os.path.isfile(binary):
            continue
        base = os.path.basename(binary)
        for value in binfmt.scan(binary, NW_HINTS, limit=3):
            finding["version_evidence"].append({"from": f"version string in {base}", "value": value})
        chrome = binfmt.scan(binary, rb"(?<![0-9.])([0-9]{2,3}\.0\.[0-9]{4}\.[0-9]{1,3})(?![0-9.])", limit=12)
        if chrome and not finding["payload"].get("chromium"):
            finding["payload"]["chromium_candidates"] = chrome
        strings = binfmt.pe_version_strings(binary)
        if strings:
            finding["payload"].setdefault("pe_versions", {})[base] = strings


def nw_payload(directory, names):
    package_nw = os.path.join(directory, "package.nw")
    if os.path.isdir(package_nw) and os.path.isfile(os.path.join(package_nw, "package.json")):
        return ("directory", package_nw)
    if os.path.isfile(package_nw):
        return ("zip", package_nw)
    if os.path.isfile(os.path.join(directory, "package.json")):
        return ("beside", directory)
    return (None, None)


def detect_nwjs_dir(directory, names):
    lower = {name.lower(): name for name in names}
    windows = "nw.dll" in lower
    linux = os.path.isfile(os.path.join(directory, "lib", "libnw.so"))
    if not (windows or linux):
        return None
    finding = finding_base("nwjs", "windows" if windows else "linux", directory)
    kind, where = nw_payload(directory, names)
    exes = _exes(directory, names, kinds=("pe", "elf"))
    program = next((name for _, name, _ in exes if name.lower() not in ("notification_helper.exe", "nwjc.exe", "chromedriver.exe", "payload.exe", "nwjc", "chromedriver", "chrome_crashpad_handler")), None)
    finding["payload"] = {"layout": kind, "where": where, "program": program}
    binaries = [os.path.join(directory, lower["nw.dll"])] if windows else [os.path.join(directory, "lib", "libnw.so")]
    if program:
        binaries.append(os.path.join(directory, program))
    nw_version_evidence(finding, binaries)
    finding["payload"]["sdk"] = "nwjc.exe" in lower or "nwjc" in lower
    if kind is None:
        finding["reasons"].append("payload-not-found")
        finding["verdict"] = "rehostable-with-work"
        finding["notes"].append("no package.nw or package.json beside the program: the payload may be appended to the program")
    elif kind in ("directory", "beside"):
        describe_web_payload(finding, Payload(directory=where), None)
    else:
        try:
            with zipfile.ZipFile(where) as archive:
                package = json.loads(archive.read("package.json").decode("utf-8-sig"))
                finding["payload"].update({"name": package.get("name"), "app_version": package.get("version"), "main": package.get("main")})
                finding["payload"]["native_modules"] = {"total": sum(1 for name in archive.namelist() if name.endswith(".node"))}
        except (zipfile.BadZipFile, KeyError, ValueError, OSError):
            finding["notes"].append("package.nw could not be read as a zip archive")
    if finding["version_evidence"]:
        finding["version"] = finding["version_evidence"][0]["value"]
    return finding


def detect_nwjs_app(bundle):
    framework = os.path.join(bundle, "Contents", "Frameworks", "nwjs Framework.framework")
    if not os.path.isdir(framework):
        return None
    finding = finding_base("nwjs", "macos", bundle)
    resources = os.path.join(bundle, "Contents", "Resources")
    app_nw = os.path.join(resources, "app.nw")
    finding["payload"] = {"layout": "directory" if os.path.isdir(app_nw) else ("zip" if os.path.isfile(app_nw) else None), "where": app_nw}
    for candidate in ("Versions/A/nwjs Framework", "nwjs Framework", "Versions/Current/nwjs Framework"):
        path = os.path.join(framework, candidate)
        if os.path.isfile(path):
            nw_version_evidence(finding, [path])
            break
    for candidate in ("Versions/A/Resources/Info.plist", "Resources/Info.plist"):
        info = _plist(os.path.join(framework, candidate))
        if info:
            finding["payload"]["framework_plist"] = {key: str(info.get(key)) for key in ("CFBundleVersion", "CFBundleShortVersionString") if info.get(key)}
            break
    if os.path.isdir(app_nw):
        describe_web_payload(finding, Payload(directory=app_nw), None)
    else:
        finding["reasons"].append("payload-not-found")
    if finding["version_evidence"]:
        finding["version"] = finding["version_evidence"][0]["value"]
    return finding


def resolve_nw_version(finding, versions_index):
    """NW.js binaries do not carry their own version in a fixed string;
    Chromium's version is there, and nwjs.io publishes which NW.js release
    was built on which Chromium."""
    if finding.get("version") or not versions_index:
        return
    candidates = finding["payload"].get("chromium_candidates") or []
    table = {}
    for row in versions_index.get("versions", []):
        chromium = (row.get("components") or {}).get("chromium")
        if chromium:
            table.setdefault(chromium, []).append(row["version"].lstrip("v"))
    for chromium in candidates:
        if chromium in table:
            finding["payload"]["chromium"] = chromium
            finding["version"] = sorted(table[chromium], key=lambda v: [int(x) for x in re.findall(r"\d+", v)])[-1]
            finding["version_evidence"].append({"from": f"Chromium {chromium} in the binary, looked up in nwjs.io/versions.json", "value": finding["version"], "all": table[chromium]})
            return
    finding["reasons"].append("runtime-version-unknown")
    finding["verdict"] = "rehostable-with-work"


# ---------------------------------------------------------------------- .NET

WINDOWS_ONLY_FRAMEWORKS = ("Microsoft.WindowsDesktop.App",)
WINDOWS_UI_ASSEMBLIES = ("presentationframework.dll", "system.windows.forms.dll", "microsoft.winui.dll", "microsoft.windowsappsdk.dll", "microsoft.maui.dll", "winrt.runtime.dll")


def _dotnet_config(directory, name):
    try:
        with open(os.path.join(directory, name), "rb") as handle:
            config = json.loads(handle.read().decode("utf-8-sig"))
    except (OSError, ValueError):
        return None
    options = config.get("runtimeOptions", {}) if isinstance(config, dict) else {}
    listed = list(options.get("frameworks") or ([options["framework"]] if options.get("framework") else [])) + list(options.get("includedFrameworks") or [])
    return options, [item.get("name") for item in listed if isinstance(item, dict)]


def detect_dotnet_dir(directory, names):
    configs = [name for name in names if name.endswith(".runtimeconfig.json")]
    if not configs:
        return None
    lower = {name.lower() for name in names}
    # The application's own configuration, not a tool's that travels with it.
    configs.sort(key=lambda name: (0 if (name[: -len(".runtimeconfig.json")] + ".exe").lower() in lower else 1, len(name)))
    parsed = [(name, _dotnet_config(directory, name)) for name in configs[:8]]
    parsed = [(name, value) for name, value in parsed if value]
    if not parsed:
        return None
    config_name, (options, _) = parsed[0]
    skipped = None
    if any(framework in WINDOWS_ONLY_FRAMEWORKS for framework in parsed[0][1][1]):
        # A Windows-only front end (a tray program) beside a portable
        # program of the same application: the portable one is the payload.
        for name, (other, frameworks) in parsed[1:]:
            if not any(framework in WINDOWS_ONLY_FRAMEWORKS for framework in frameworks) and (name[: -len(".runtimeconfig.json")] + ".exe").lower() in lower:
                skipped = config_name
                config_name, options = name, other
                break
    base = config_name[: -len(".runtimeconfig.json")]
    exes = _exes(directory, names, kinds=("pe", "elf", "macho"))
    platform = PLATFORM_OF[exes[0][2][0]] if exes else ("windows" if any(n.endswith(".dll") and n.lower() in ("coreclr.dll", "hostfxr.dll") for n in names) else "unknown")
    finding = finding_base("dotnet", platform, directory)
    frameworks = list(options.get("frameworks") or ([options["framework"]] if options.get("framework") else []))
    included = list(options.get("includedFrameworks") or [])
    names_of = [item.get("name") for item in frameworks + included if isinstance(item, dict)]
    core = next((item for item in frameworks + included if item.get("name") == "Microsoft.NETCore.App"), None)
    finding["version"] = (core or {}).get("version")
    finding["version_evidence"].append({"from": config_name, "value": finding["version"]})
    finding["payload"] = {
        "assembly": base + ".dll" if (base + ".dll").lower() in lower else None,
        "runtimeconfig": config_name,
        "tfm": options.get("tfm"),
        "frameworks": names_of,
        "self_contained": bool(included),
        "deps": base + ".deps.json" if (base + ".deps.json").lower() in lower else None,
        "avalonia": "avalonia.dll" in lower or "avalonia.base.dll" in lower,
        "aspnet": "Microsoft.AspNetCore.App" in names_of or "microsoft.aspnetcore.dll" in lower,
    }
    windows_ui = [name for name in WINDOWS_UI_ASSEMBLIES[2:5] if name in lower]
    if skipped:
        finding["reasons"].append("windows-front-end-skipped")
        finding["notes"].append(f"{skipped[: -len('.runtimeconfig.json')]} is a Windows-only front end (Windows desktop framework); {base} beside it is portable and is what gets re-hosted")
    if any(name in WINDOWS_ONLY_FRAMEWORKS for name in names_of) or (windows_ui and not finding["payload"]["avalonia"]):
        finding["verdict"] = "not-rehostable"
        finding["route"] = "wine"
        finding["reasons"].append("dotnet-windows-desktop")
        finding["notes"].append("uses the Windows desktop framework (WPF or Windows Forms" + (": " + ", ".join(windows_ui) if windows_ui else "") + "): there is no Linux build of it, so this goes to Wine")
    natives = []
    for name in names:
        path = os.path.join(directory, name)
        if name.lower().endswith((".dll", ".so", ".dylib")) and os.path.isfile(path):
            info = binfmt.pe_info(path) if name.lower().endswith(".dll") else None
            if info and not info["clr"]:
                natives.append({"path": name, "format": "pe", "archs": [info["machine"]], "size": info["size"]})
            elif not info:
                kind = binfmt.binary_kind(path)
                if kind and kind[0] != "pe":
                    natives.append({"path": name, "format": kind[0], "archs": kind[1], "size": os.path.getsize(path)})
    finding["natives"] = natives
    finding["payload"]["native_libraries"] = len(natives)
    if finding["payload"]["assembly"] is None:
        finding["reasons"].append("main-assembly-not-found")
        finding["verdict"] = "rehostable-with-work" if finding["verdict"] == "rehostable" else finding["verdict"]
    return finding


def detect_dotnet_bundle(path):
    bundle = binfmt.dotnet_bundle_read(path)
    if not bundle:
        return None
    kind = binfmt.binary_kind(path)
    finding = finding_base("dotnet", PLATFORM_OF[kind[0]] if kind else "unknown", os.path.dirname(path))
    names = [entry["path"] for entry in bundle["files"]]
    configs = [name for name in names if name.endswith(".runtimeconfig.json")]
    finding["payload"] = {"single_file": os.path.basename(path), "bundle_version": bundle["version"], "files": len(names), "runtimeconfig": configs[0] if configs else None}
    lower = {name.lower() for name in names}
    finding["payload"]["avalonia"] = "avalonia.dll" in lower or "avalonia.base.dll" in lower
    finding["payload"]["self_contained"] = "system.private.corelib.dll" in lower
    windows_ui = [name for name in WINDOWS_UI_ASSEMBLIES if name in lower]
    if windows_ui and not finding["payload"]["avalonia"]:
        finding["verdict"] = "not-rehostable"
        finding["route"] = "wine"
        finding["reasons"].append("dotnet-windows-desktop")
    else:
        finding["verdict"] = "rehostable-with-work"
    finding["reasons"].append("dotnet-single-file")
    finding["notes"].append("a single-file bundle: the assemblies are inside the program and are extracted without running it")
    return finding


def detect_dotnet_framework(directory, names):
    """.NET Framework programs (the Windows-only predecessor): an .exe with
    a CLR header and no runtimeconfig.json."""
    for _, name, kind in _exes(directory, names, kinds=("pe",))[:6]:
        info = binfmt.pe_info(os.path.join(directory, name))
        if info and info["clr"]:
            finding = finding_base("dotnet-framework", "windows", directory)
            finding["verdict"] = "not-rehostable"
            finding["route"] = "wine"
            finding["reasons"].append("dotnet-framework")
            finding["payload"] = {"program": name}
            target = binfmt.scan(os.path.join(directory, name), rb"\.NETFramework,Version=v([0-9.]+)", limit=1)
            finding["version"] = target[0] if target else None
            finding["notes"].append("a .NET Framework program: Windows-only; Mono was not tested, so this goes to Wine")
            return finding
    return None


# ----------------------------------------------------------------------- JVM

JAR_NATIVE = re.compile(r"\.(dll|so|dylib|jnilib)$", re.I)


def jar_facts(path, limit=20000):
    """Main class, class-file version and platform libraries of a jar (or of
    a program with a jar appended)."""
    facts = {"main_class": None, "class_major": None, "natives": {"windows": 0, "linux": 0, "macos": 0}, "swt": None, "javafx": False, "entries": 0}
    try:
        with zipfile.ZipFile(path) as archive:
            names = archive.namelist()
            facts["entries"] = len(names)
            if "META-INF/MANIFEST.MF" in names:
                manifest = archive.read("META-INF/MANIFEST.MF").decode("utf-8", "replace").replace("\r\n ", "").replace("\n ", "")
                for line in manifest.splitlines():
                    key, _, value = line.partition(":")
                    key = key.strip().lower()
                    if key == "main-class":
                        facts["main_class"] = value.strip()
                    elif key == "class-path":
                        facts["class_path"] = value.strip()[:2000]
                    elif key in ("launcher-agent-class", "multi-release", "automatic-module-name", "build-jdk", "build-jdk-spec", "created-by"):
                        facts[key] = value.strip()[:80]
            first_class = None
            for name in names[:limit]:
                lower = name.lower()
                if JAR_NATIVE.search(lower):
                    if lower.endswith(".dll"):
                        facts["natives"]["windows"] += 1
                    elif lower.endswith(".so"):
                        facts["natives"]["linux"] += 1
                    else:
                        facts["natives"]["macos"] += 1
                    if "swt-" in lower and facts["swt"] is None:
                        facts["swt"] = {"library": name}
                if name.startswith("org/eclipse/swt/") and facts["swt"] is None:
                    facts["swt"] = {"library": None}
                if name.startswith("javafx/") or name.startswith("com/sun/javafx/"):
                    facts["javafx"] = True
                if first_class is None and name.endswith(".class") and "module-info" not in name and not name.startswith("META-INF/"):
                    first_class = name
            main = facts["main_class"].replace(".", "/") + ".class" if facts["main_class"] else None
            for candidate in (main, first_class):
                if candidate and candidate in names:
                    facts["class_major"] = binfmt.java_class_major(archive.read(candidate)[:8])
                    break
            if facts["swt"] is not None and "version.txt" in names:
                facts["swt"]["version_txt"] = archive.read("version.txt").decode("utf-8", "replace").strip()[:60]
            if facts["swt"] is not None:
                fragment = [name for name in names if name.startswith("org/eclipse/swt/internal/") and "/win32/" in name]
                gtk = [name for name in names if name.startswith("org/eclipse/swt/internal/gtk")]
                cocoa = [name for name in names if name.startswith("org/eclipse/swt/internal/cocoa")]
                facts["swt"]["port"] = "win32" if fragment else ("gtk" if gtk else ("cocoa" if cocoa else "unknown"))
    except (zipfile.BadZipFile, OSError, KeyError):
        return None
    return facts


def java_major_from_class(major):
    return major - 44 if major and major >= 45 else None


def parse_jpackage_cfg(path):
    out = {"classpath": [], "options": [], "mainclass": None, "mainjar": None, "mainmodule": None}
    section = ""
    text = binfmt.text_or_none(binfmt.read_at(path, 0, 1 << 20)) or ""
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("["):
            section = line.strip("[]")
            continue
        key, _, value = line.partition("=")
        if not _:
            continue
        if key == "app.classpath":
            out["classpath"].append(value)
        elif key == "app.mainclass":
            out["mainclass"] = value
        elif key == "app.mainjar":
            out["mainjar"] = value
        elif key == "app.mainmodule":
            out["mainmodule"] = value
        elif key == "java-options" and section == "JavaOptions":
            out["options"].append(value)
    return out


def java_release(runtime_dir):
    path = os.path.join(runtime_dir, "release")
    if os.path.isfile(path):
        text = binfmt.text_or_none(binfmt.read_at(path, 0, 8192)) or ""
        match = re.search(r'JAVA_VERSION="([^"]+)"', text)
        modules = re.search(r'MODULES="([^"]+)"', text)
        return {"version": match.group(1) if match else None, "modules": modules.group(1).split() if modules else []}
    return None


def detect_jvm_dir(directory, names, depth):
    """A jpackage image (app/*.cfg beside runtime/), or jars beside a
    launcher and a bundled Java runtime."""
    finding = None
    app_dir = None
    for candidate in ("app", os.path.join("Contents", "app"), os.path.join("lib", "app")):
        path = os.path.join(directory, candidate)
        if os.path.isdir(path) and any(name.endswith(".cfg") for name in os.listdir(path)):
            app_dir = path
            break
    if app_dir:
        cfg_name = sorted(name for name in os.listdir(app_dir) if name.endswith(".cfg"))[0]
        cfg = parse_jpackage_cfg(os.path.join(app_dir, cfg_name))
        if cfg["mainclass"] or cfg["mainjar"] or cfg["mainmodule"]:
            exes = _exes(directory, names, kinds=("pe", "elf", "macho"))
            platform = "macos" if "Contents" in app_dir.split(os.sep) else (PLATFORM_OF[exes[0][2][0]] if exes else "unknown")
            finding = finding_base("jvm", platform, directory)
            finding["payload"] = {"wrapper": "jpackage", "cfg": os.path.join(app_dir, cfg_name), "app": app_dir, **cfg}
            for runtime in ("runtime", os.path.join("Contents", "runtime", "Contents", "Home"), os.path.join("lib", "runtime")):
                release = java_release(os.path.join(directory, runtime))
                if release:
                    finding["payload"]["bundled_runtime"] = release
                    finding["version"] = release["version"]
                    finding["version_evidence"].append({"from": f"{runtime}/release", "value": release["version"]})
                    break
    if finding is None:
        jars = [name for name in names if name.lower().endswith(".jar")]
        if not jars:
            return None
        if depth > 3 and not any(os.path.isdir(os.path.join(directory, name)) and name.lower() in ("jre", "runtime", "jdk") for name in names):
            return None
        exes = _exes(directory, names, kinds=("pe", "elf", "macho"))
        with_main = []
        for name in jars[:40]:
            facts = jar_facts(os.path.join(directory, name))
            if facts and facts["main_class"]:
                with_main.append((os.path.getsize(os.path.join(directory, name)), name))
        if not with_main:
            return None
        platform = PLATFORM_OF[exes[0][2][0]] if exes else "any"
        finding = finding_base("jvm", platform, directory)
        finding["payload"] = {"wrapper": "launcher-beside-jar" if exes else "plain-jar", "mainjar": sorted(with_main, reverse=True)[0][1], "app": directory, "classpath": [], "options": []}
        for runtime in ("jre", "runtime", "jdk"):
            release = java_release(os.path.join(directory, runtime))
            if release:
                finding["payload"]["bundled_runtime"] = release
                finding["version"] = release["version"]
                finding["version_evidence"].append({"from": f"{runtime}/release", "value": release["version"]})
                break
        for config in ("config.json",):
            path = os.path.join(directory, config)
            if os.path.isfile(path):
                try:
                    packr = json.loads(binfmt.read_at(path, 0, 1 << 20).decode("utf-8-sig"))
                    if isinstance(packr, dict) and "mainClass" in packr:
                        finding["payload"]["wrapper"] = "packr"
                        finding["payload"]["mainclass"] = packr.get("mainClass")
                        finding["payload"]["classpath"] = list(packr.get("classPath") or [])
                        finding["payload"]["options"] = list(packr.get("vmArgs") or [])
                except ValueError:
                    pass
    describe_jars(finding)
    return finding


def describe_jars(finding):
    app = finding["payload"]["app"]
    totals = {"jars": 0, "windows": 0, "linux": 0, "macos": 0}
    swt = None
    javafx_platform_jars = []
    class_major = None
    for root, _, names in os.walk(app):
        for name in names:
            if not name.lower().endswith(".jar"):
                continue
            totals["jars"] += 1
            if totals["jars"] > 400:
                break
            facts = jar_facts(os.path.join(root, name))
            if not facts:
                continue
            for key in ("windows", "linux", "macos"):
                totals[key] += facts["natives"][key]
            if facts["swt"] and (swt is None or facts["swt"].get("library")):
                swt = dict(facts["swt"], jar=os.path.relpath(os.path.join(root, name), app))
            if re.match(r"javafx[-.].*-(win|mac|linux)(-[a-z0-9]+)?\.jar$", name):
                javafx_platform_jars.append(name)
            if name == finding["payload"].get("mainjar") or class_major is None:
                class_major = facts["class_major"] or class_major
    finding["payload"]["jar_natives"] = totals
    finding["payload"]["class_major"] = class_major
    finding["payload"]["needs_java"] = java_major_from_class(class_major)
    if not finding["version"] and class_major:
        finding["version"] = str(java_major_from_class(class_major))
        finding["version_evidence"].append({"from": "class-file version of the main jar (a minimum)", "value": finding["version"]})
    if swt:
        finding["payload"]["swt"] = swt
        if swt.get("port") != "gtk":
            finding["reasons"].append("swt-platform-port")
            finding["verdict"] = "rehostable-with-work"
            finding["notes"].append(f"uses SWT's {swt.get('port')} port: the GTK port of the same SWT version has to replace it")
    if javafx_platform_jars:
        finding["payload"]["javafx_platform_jars"] = javafx_platform_jars
        if not any("-linux" in name for name in javafx_platform_jars):
            finding["reasons"].append("javafx-platform-jars")
            finding["verdict"] = "rehostable-with-work"
            finding["notes"].append("carries JavaFX jars for another platform: the Linux jars of the same version replace them")
    modules = (finding["payload"].get("bundled_runtime") or {}).get("modules") or []
    if any(module.startswith("javafx.") for module in modules):
        finding["reasons"].append("javafx-in-runtime-image")
        finding["verdict"] = "rehostable-with-work"
        finding["notes"].append("JavaFX is linked into the bundled runtime image: the Linux JavaFX modules of the same version are needed")
    if finding["payload"].get("mainmodule") and totals["jars"] == 0:
        finding["reasons"].append("application-in-runtime-image")
        finding["verdict"] = "rehostable-with-work"
        finding["notes"].append("the application's classes are inside the runtime image (jlink): they have to be extracted with jimage")
    if totals["windows"] and not totals["linux"] and finding["platform"] == "windows":
        finding["reasons"].append("jni-libraries-windows-only")
        if finding["verdict"] == "rehostable":
            finding["verdict"] = "rehostable-with-work"


def detect_launch4j(path):
    """A Windows program with a jar appended (launch4j, JSmooth, or a jar
    made executable)."""
    tail = binfmt.zip_tail(path)
    if not tail:
        return None
    facts = jar_facts(path)
    if not facts or not (facts["main_class"] or facts["class_major"]):
        return None
    finding = finding_base("jvm", "windows", os.path.dirname(path))
    wrapper = "launch4j" if binfmt.contains(path, b"launch4j") or binfmt.contains(path, "Launch4j".encode("utf-16-le")) else "jar-in-exe"
    finding["payload"] = {"wrapper": wrapper, "carrier": os.path.basename(path), "jar_start": tail["start"], "mainclass": facts["main_class"], "class_major": facts["class_major"], "needs_java": java_major_from_class(facts["class_major"]), "jar_natives": facts["natives"]}
    if facts["class_major"]:
        finding["version"] = str(java_major_from_class(facts["class_major"]))
        finding["version_evidence"].append({"from": "class-file version of the main class (a minimum)", "value": finding["version"]})
    if facts["swt"]:
        finding["payload"]["swt"] = dict(facts["swt"], jar=os.path.basename(path))
        if facts["swt"].get("port") != "gtk":
            finding["reasons"].append("swt-platform-port")
            finding["verdict"] = "rehostable-with-work"
            finding["notes"].append(f"uses SWT's {facts['swt'].get('port')} port: the GTK port of the same SWT version has to replace it")
    return finding


# --------------------------------------------------------------------- Godot


def describe_pck(finding, carrier, located):
    start, size, how = located
    info = binfmt.pck_read(carrier, start)
    if not info:
        return False
    finding["version"] = info["engine"]
    finding["version_evidence"].append({"from": f"pack header ({how}) in {os.path.basename(carrier)}", "value": info["engine"]})
    files = info.get("files") or []
    extensions = [entry["path"] for entry in files if entry["path"].endswith((".gdextension", ".gdnlib", ".gdns"))]
    csharp = any(entry["path"].endswith((".cs", ".csproj")) or "/.mono/" in entry["path"] for entry in files)
    finding["payload"].update(
        {
            "carrier": carrier,
            "pack": {"how": how, "start": start, "size": size, "format": info["format"], "flags": info["flags"], "files": info.get("file_count"), "encrypted_directory": info["encrypted"]},
            "encrypted_files": sum(1 for entry in files if entry["flags"] & 1),
            "native_extensions": extensions[:20],
            "csharp": csharp,
        }
    )
    if info["encrypted"] or finding["payload"]["encrypted_files"]:
        finding["reasons"].append("pack-encrypted")
        finding["verdict"] = "not-rehostable"
        finding["route"] = "wine"
        finding["notes"].append("the pack is encrypted with a key compiled into the publisher's engine build: the stock engine cannot read it")
    if extensions:
        finding["reasons"].append("godot-native-extension")
        if finding["verdict"] == "rehostable":
            finding["verdict"] = "rehostable-with-work"
        finding["notes"].append(f"{len(extensions)} native extension(s) (GDExtension or GDNative): each needs its Linux library")
    return True


def detect_godot_dir(directory, names):
    lower = {name.lower(): name for name in names}
    packs = [name for name in names if name.lower().endswith(".pck") and binfmt.read_at(os.path.join(directory, name), 0, 4) == binfmt.PCK_MAGIC]
    exes = _exes(directory, names, kinds=("pe", "elf", "macho"))
    finding = None
    if packs:
        platform = PLATFORM_OF[exes[0][2][0]] if exes else "any"
        finding = finding_base("godot", platform, directory)
        carrier = os.path.join(directory, packs[0])
        finding["payload"] = {"program": exes[0][1] if exes else None}
        if not describe_pck(finding, carrier, (0, os.path.getsize(carrier), "standalone")):
            return None
    else:
        for _, name, kind in exes[:4]:
            carrier = os.path.join(directory, name)
            located = binfmt.pck_locate(carrier)
            if located:
                finding = finding_base("godot", PLATFORM_OF[kind[0]], directory)
                finding["payload"] = {"program": name}
                if not describe_pck(finding, carrier, located):
                    finding = None
                break
    if finding is None:
        return None
    mono = [name for name in names if name.lower().startswith("data_") and os.path.isdir(os.path.join(directory, name))]
    if mono or finding["payload"].get("csharp"):
        finding["payload"]["mono_data"] = mono
        finding["reasons"].append("godot-dotnet")
        finding["verdict"] = "rehostable-with-work" if finding["verdict"] == "rehostable" else finding["verdict"]
        finding["notes"].append("a C# (Godot .NET) game: needs the .NET build of the engine and the game's assemblies re-resolved for Linux; not attempted")
    libraries = [name for name in names if name.lower().endswith((".dll", ".dylib", ".so")) and os.path.isfile(os.path.join(directory, name))]
    finding["payload"]["libraries_beside"] = libraries[:20]
    return finding


# ---------------------------------------------------------------- Ren'Py, LÖVE


def renpy_version(base):
    for name in ("vc_version.py", "__init__.py"):
        path = os.path.join(base, "renpy", name)
        if not os.path.isfile(path):
            continue
        text = binfmt.text_or_none(binfmt.read_at(path, 0, 200000)) or ""
        match = re.search(r"""^version\s*=\s*u?['"]([0-9][0-9.]+)""", text, re.M)
        if match:
            return match.group(1), f"renpy/{name}"
        match = re.search(r"version_tuple\s*=\s*\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)", text)
        if match:
            return ".".join(match.groups()), f"renpy/{name}"
    return None, None


def detect_renpy_dir(directory, names):
    if not (os.path.isdir(os.path.join(directory, "renpy")) and os.path.isdir(os.path.join(directory, "game"))):
        return None
    lib = os.path.join(directory, "lib")
    platforms = sorted(os.listdir(lib)) if os.path.isdir(lib) else []
    has = {
        "windows": any("windows" in name for name in platforms),
        "linux": any("linux-x86_64" in name for name in platforms),
        "macos": any(name.startswith(("py3-mac", "py2-mac", "mac-", "darwin-")) for name in platforms),
    }
    in_bundle = f"{os.sep}Contents{os.sep}" in directory + os.sep
    platform = "macos" if in_bundle or (has["macos"] and not has["windows"]) else ("windows" if has["windows"] else ("linux" if has["linux"] else "unknown"))
    finding = finding_base("renpy", platform, directory)
    version, source = renpy_version(directory)
    finding["version"] = version
    if version:
        finding["version_evidence"].append({"from": source, "value": version})
    else:
        finding["reasons"].append("runtime-version-unknown")
        finding["verdict"] = "rehostable-with-work"
    game = os.path.join(directory, "game")
    game_files = os.listdir(game)
    finding["payload"] = {
        "base": directory,
        "lib_platforms": platforms,
        "carries_linux_runtime": has["linux"],
        "archives": sum(1 for name in game_files if name.endswith(".rpa")),
        "compiled_scripts": sum(1 for name in game_files if name.endswith(".rpyc")),
        "python": "3" if any(name.startswith("py3-") or name.startswith("python3") for name in platforms) else "2",
    }
    if has["linux"]:
        finding["reasons"].append("package-carries-linux-runtime")
        finding["notes"].append("this package already carries Ren'Py's Linux runtime: it only has to be started")
    return finding


def love_conf_version(archive):
    try:
        conf = archive.read("conf.lua").decode("utf-8", "replace")
    except KeyError:
        return None
    match = re.search(r"""\.version\s*=\s*['"]([0-9.]+)['"]""", conf)
    return match.group(1) if match else None


def detect_love_file(path):
    """A fused LÖVE game (love.exe with the game's zip appended) or a .love."""
    name = os.path.basename(path)
    is_love = name.lower().endswith(".love")
    tail = binfmt.zip_tail(path)
    if not tail:
        return None
    try:
        with zipfile.ZipFile(path) as archive:
            names = set(archive.namelist())
            if "main.lua" not in names:
                return None
            conf = love_conf_version(archive)
    except (zipfile.BadZipFile, OSError):
        return None
    kind = binfmt.binary_kind(path)
    finding = finding_base("love", PLATFORM_OF[kind[0]] if kind else "any", os.path.dirname(path))
    finding["payload"] = {"carrier": name, "fused": not is_love and tail["start"] > 0, "zip_start": tail["start"], "zip_end": tail["end"], "entries": tail["entries"], "conf_version": conf}
    directory = os.path.dirname(path)
    for candidate in ("love.dll", name):
        binary = os.path.join(directory, candidate)
        if os.path.isfile(binary) and binfmt.binary_kind(binary):
            strings = binfmt.pe_version_strings(binary)
            value = strings.get("ProductVersion") or strings.get("FileVersion")
            if value and re.match(r"\d+\.\d+", value):
                finding["version_evidence"].append({"from": f"version resource of {candidate}", "value": ".".join(re.findall(r"\d+", value)[:3])})
            found = binfmt.scan(binary, rb"LOVE ([0-9]+\.[0-9]+(?:\.[0-9]+)?)", limit=2)
            for value in found:
                finding["version_evidence"].append({"from": f"version string in {candidate}", "value": value})
    if conf:
        finding["version_evidence"].append({"from": "t.version in the game's conf.lua (what the game was written for)", "value": conf})
    if finding["version_evidence"]:
        finding["version"] = finding["version_evidence"][0]["value"]
    else:
        finding["reasons"].append("runtime-version-unknown")
        finding["verdict"] = "rehostable-with-work"
    return finding


def detect_love_app(bundle):
    resources = os.path.join(bundle, "Contents", "Resources")
    if not os.path.isdir(os.path.join(bundle, "Contents", "Frameworks", "love.framework")) and not os.path.isdir(os.path.join(bundle, "Contents", "Frameworks", "Lua.framework")):
        return None
    loves = [name for name in os.listdir(resources) if name.lower().endswith(".love")] if os.path.isdir(resources) else []
    if not loves:
        return None
    finding = detect_love_file(os.path.join(resources, loves[0]))
    if finding is None:
        return None
    finding["platform"] = "macos"
    finding["app_dir"] = bundle
    info = _plist(os.path.join(bundle, "Contents", "Info.plist"))
    for candidate in ("Versions/A/Resources/Info.plist", "Resources/Info.plist"):
        framework = _plist(os.path.join(bundle, "Contents", "Frameworks", "love.framework", candidate))
        version = framework.get("CFBundleShortVersionString") or framework.get("CFBundleVersion")
        if version:
            finding["version_evidence"].insert(0, {"from": "love.framework Info.plist", "value": str(version)})
            finding["version"] = str(version)
            break
    finding["payload"]["bundle_version"] = str(info.get("CFBundleShortVersionString") or "")
    return finding


# --------------------------------------------------------------- PyInstaller


def detect_pyinstaller(path):
    archive = binfmt.pyinstaller_read(path)
    if not archive:
        return None
    kind = binfmt.binary_kind(path)
    finding = finding_base("pyinstaller", PLATFORM_OF[kind[0]] if kind else "unknown", os.path.dirname(path))
    entries = archive["entries"]
    scripts = [entry["name"] for entry in entries if entry["type"] == "s"]
    binaries = [entry["name"] for entry in entries if entry["type"] == "b"]
    extensions = [name for name in binaries if name.lower().endswith((".pyd", ".so")) or ".cpython-" in name or ".abi3." in name]
    metadata = sorted({name.replace("\\", "/").split("/")[0] for entry in entries for name in [entry["name"]] if ".dist-info" in name.replace("\\", "/").split("/")[0]})
    internal = os.path.join(os.path.dirname(path), "_internal")
    onedir = os.path.isdir(internal)
    if onedir:
        for root, _, names in os.walk(internal):
            for name in names:
                rel = os.path.relpath(os.path.join(root, name), internal).replace(os.sep, "/")
                if name.lower().endswith((".pyd", ".so")) or ".cpython-" in name:
                    extensions.append(rel)
                if rel.split("/")[0].endswith(".dist-info") and rel.split("/")[0] not in metadata:
                    metadata.append(rel.split("/")[0])
    pyz = next((entry for entry in entries if entry["type"] == "z"), None)
    finding["version"] = archive["python"]
    finding["version_evidence"].append({"from": f"CArchive cookie ({archive['pylib']})", "value": archive["python"]})
    finding["payload"] = {
        "carrier": os.path.basename(path),
        "mode": "onedir" if onedir else "onefile",
        "scripts": scripts,
        "entries": len(entries),
        "extension_modules": sorted(extensions)[:200],
        "extension_count": len(extensions),
        "other_binaries": len(binaries) - len([name for name in binaries if name in extensions]),
        "metadata": metadata[:200],
        "has_pyz": pyz is not None,
    }
    finding["verdict"] = "rehostable-with-work"
    finding["reasons"].append("python-frozen")
    finding["notes"].append("frozen Python: the bytecode is portable within one Python minor version; every extension module needs its Linux build")
    return finding


# -------------------------------------------------------- native, not re-hostable

NATIVE_MARKERS = [
    ("qt", ("qt6core.dll", "qt5core.dll", "qtcore4.dll", "qt6widgets.dll", "qtcore.framework", "qt6gui.dll", "libqt5core.so.5", "libqt6core.so.6")),
    ("flutter", ("flutter_windows.dll", "fluttermacos.framework", "flutter_assets", "libflutter_linux_gtk.so", "app.framework")),
    ("unity", ("unityplayer.dll", "unityplayer.dylib", "unityplayer.so", "unitycrashhandler64.exe")),
    ("unreal", ("ue4prereqsetup_x64.exe", "ueprereqsetup_x64.exe", "engine")),
    ("cef", ("libcef.dll", "chromium embedded framework.framework", "libcef.so")),
    ("webview2", ("webview2loader.dll", "microsoft.web.webview2.core.dll")),
    ("gamemaker", ("data.win", "game.ios", "game.unx")),
    ("wxwidgets", ("wxbase32u_vc_custom.dll", "wxmsw32u_core_vc_custom.dll")),
    ("gtk", ("libgtk-3-0.dll", "libgtk-4-1.dll", "gtk-3-vs17.dll")),
    ("sdl", ("sdl2.dll", "sdl3.dll")),
    ("uwp-msix", ("appxmanifest.xml",)),
]


def native_toolkits(all_names):
    lower = {name.lower() for name in all_names}
    found = []
    for toolkit, markers in NATIVE_MARKERS:
        hits = [marker for marker in markers if marker in lower]
        if toolkit == "unreal":
            hits = [marker for marker in hits if marker != "engine"] or (["engine"] if "engine" in lower and any(name.endswith("-win64-shipping.exe") for name in lower) else [])
        if toolkit == "flutter" and hits == ["app.framework"]:
            hits = []
        if hits:
            found.append({"toolkit": toolkit, "markers": hits})
    return found


def detect_native(root, all_names, programs):
    toolkits = native_toolkits(all_names)
    program = programs[0] if programs else None
    tauri = False
    if program and any(t["toolkit"] == "webview2" for t in toolkits) or (program and not toolkits):
        tauri = program is not None and (binfmt.contains(program[1], b"tauri") and binfmt.contains(program[1], b"wry"))
    if tauri:
        toolkits.insert(0, {"toolkit": "tauri", "markers": ["'tauri' and 'wry' in the program"]})
    if not toolkits and not program:
        return None
    primary = toolkits[0]["toolkit"] if toolkits else "unknown"
    kind = program[2] if program else None
    finding = finding_base(f"native:{primary}", PLATFORM_OF[kind[0]] if kind else "unknown", root)
    finding["verdict"] = "not-rehostable"
    finding["route"] = {"windows": "wine", "macos": "none", "linux": "native"}.get(finding["platform"], "unknown")
    finding["reasons"].append("native-program")
    finding["payload"] = {"toolkits": toolkits, "program": os.path.relpath(program[1], root) if program else None}
    words = {
        "qt": "a native Qt program",
        "flutter": "a Flutter program (compiled ahead of time for one platform)",
        "tauri": "a Tauri program (a native Rust host around the system web view)",
        "unity": "a Unity game (the player and IL2CPP or Mono code are built for one platform)",
        "unreal": "an Unreal Engine game",
        "cef": "a native program that embeds Chromium (CEF): the host is native code",
        "webview2": "a native program around Microsoft's WebView2: the host is native code",
        "gamemaker": "a GameMaker game",
        "unknown": "a native program with no portable runtime found",
    }
    finding["notes"].append(words.get(primary, f"a native program ({primary})") + ": not re-hostable")
    return finding


# ----------------------------------------------------------------------- walk

ORDER = {"electron": 0, "nwjs": 0, "renpy": 1, "godot": 1, "love": 1, "dotnet": 2, "jvm": 2, "pyinstaller": 3, "dotnet-framework": 4}


def detect(root, max_depth=9):
    """All findings below root, the most significant first."""
    findings = []
    all_names = set()
    programs = []  # (depth, path, kind) of programs seen, for the native verdict
    claimed = []  # directories a finding owns: nothing below them is examined
    file_checks = 0
    root = os.path.abspath(root)
    for directory, dirs, names in os.walk(root):
        depth = 0 if directory == root else directory[len(root) :].count(os.sep)
        if depth > max_depth or any(directory == owner or directory.startswith(owner + os.sep) for owner in claimed):
            dirs[:] = []
            continue
        dirs[:] = sorted(name for name in dirs if name not in SKIP_DIRS and not name.endswith(".lproj") and not os.path.islink(os.path.join(directory, name)))
        all_names.update(names)
        all_names.update(dirs)
        found = []
        if directory.endswith(".app") and os.path.isdir(os.path.join(directory, "Contents")):
            for detector in (detect_electron_app, detect_nwjs_app, detect_love_app):
                finding = detector(directory)
                if finding:
                    found.append(finding)
                    break
        if not found and "resources" in dirs:
            finding = detect_electron_dir(directory, names)
            if finding:
                found.append(finding)
        if not found:
            finding = detect_nwjs_dir(directory, names)
            if finding:
                found.append(finding)
        if not found:
            finding = detect_renpy_dir(directory, names)
            if finding:
                found.append(finding)
        if not found:
            finding = detect_godot_dir(directory, names)
            if finding:
                found.append(finding)
        if not found:
            finding = detect_dotnet_dir(directory, names)
            if finding:
                found.append(finding)
        if not found:
            finding = detect_jvm_dir(directory, names, depth)
            if finding:
                found.append(finding)
        if not found and depth <= 4:
            for size, name, kind in _exes(directory, names, kinds=("pe", "elf", "macho"))[:6]:
                path = os.path.join(directory, name)
                programs.append((depth, path, kind))
                if file_checks > 40:
                    continue
                file_checks += 1
                finding = detect_pyinstaller(path) or detect_love_file(path) or detect_launch4j(path) or detect_dotnet_bundle(path)
                if finding:
                    found.append(finding)
                    break
            if not found:
                for name in names:
                    if name.lower().endswith(".love"):
                        finding = detect_love_file(os.path.join(directory, name))
                        if finding:
                            found.append(finding)
                            break
        if not found and depth <= 4:
            finding = detect_dotnet_framework(directory, names)
            if finding:
                found.append(finding)
        for finding in found:
            finding["depth"] = depth
            findings.append(finding)
            if finding["runtime"] != "dotnet-framework":
                claimed.append(directory)
                dirs[:] = []
    findings.sort(key=lambda f: (ORDER.get(f["runtime"], 5), f["depth"]))
    portable = [f for f in findings if f["runtime"] != "dotnet-framework"]
    native = None
    if not portable:
        programs.sort(key=lambda item: (item[0], -os.path.getsize(item[1])))
        native = detect_native(root, all_names, programs)
        if native and findings:  # a .NET Framework program is the better description
            native = None
        if native:
            native["depth"] = 0
            findings.append(native)
    return findings


def summary(finding):
    """One line for a person."""
    if not finding:
        return "nothing recognised"
    version = finding.get("version") or "version unknown"
    text = f"{finding['runtime']} {version}, built for {finding['platform']}: {finding['verdict']} (route: {finding['route']})"
    if finding["reasons"]:
        text += " [" + ", ".join(finding["reasons"]) + "]"
    return text
