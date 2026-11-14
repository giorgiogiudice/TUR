#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""What TUR can read from a Windows installation to carry the person's
desktop over (docs/reports/exp-inwin-1.md, section 7): wallpaper and
colours, desktop icons and where they are, shortcuts, taskbar and Start
pins, drive letters and labels, known folders, default applications,
installed applications, the account, regional settings and displays.

    carry.py --fixture DIR     put a test wallpaper and test shortcuts of
                               TUR's own on this (throwaway) desktop
    carry.py --out FILE.json   read everything, as JSON

Reading changes nothing. Each probe names where the fact lives (`where`),
so the same fact can be found on the disk when Windows is not running:
HKCU is the file NTUSER.DAT in the profile; HKLM\\SOFTWARE and HKLM\\SYSTEM
are the files of those names in Windows\\System32\\config.

Artifacts hold paths, hashes, sizes and geometry: never a Microsoft file.
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import hashlib
import json
import os
import pathlib
import re
import struct
import subprocess
import sys
import time
import winreg

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE))
from lnk import parse_lnk, parse_url  # noqa: E402
from probe import HKCR, HKCU, HKLM, HKU, Probes, powershell, reg_bytes, reg_subkeys, reg_value, reg_values, run_tool, token_facts  # noqa: E402

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
JSON = " | ConvertTo-Json -Depth 4 -Compress"
CV = r"Software\Microsoft\Windows\CurrentVersion"
DESKTOP_BAG = r"Software\Microsoft\Windows\Shell\Bags\1\Desktop"
EXTENSIONS = [".txt", ".pdf", ".docx", ".xlsx", ".jpg", ".png", ".mp3", ".mp4", ".zip", ".html", ".csv", ".json", ".xml", ".py", ".svg", ".7z", ".iso", ".msi", ".bat", ".ps1"]
PROTOCOLS = ["http", "https", "mailto", "ms-settings"]
# The per-user values compared between the running registry and a saved copy of its file.
OFFLINE = [
    (r"Control Panel\Desktop", "WallPaper"), (r"Control Panel\Desktop", "WallpaperStyle"), (r"Control Panel\Desktop", "TileWallpaper"),
    (r"Control Panel\Colors", "Background"), (r"Software\Microsoft\Windows\DWM", "AccentColor"),
    (CV + r"\Themes\Personalize", "AppsUseLightTheme"), (CV + r"\Themes\Personalize", "SystemUsesLightTheme"),
    (CV + r"\Explorer\User Shell Folders", "Desktop"), (CV + r"\Explorer\User Shell Folders", "Personal"),
    (CV + r"\Explorer\Taskband", "Favorites"), (r"Keyboard Layout\Preload", "1"), (r"Control Panel\International", "LocaleName"),
    (DESKTOP_BAG, "IconLayouts"), (DESKTOP_BAG, "FFlags"),
]


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def file_facts(path):
    path = os.path.expandvars(str(path))
    if not os.path.isfile(path):
        return {"path": path, "exists": False}
    return {"path": path, "exists": True, "bytes": os.path.getsize(path), "sha256": sha256(path)}


def my_sid():
    code, text = run_tool(["whoami.exe", "/user", "/fo", "csv", "/nh"])
    return text.strip().rsplit(",", 1)[-1].strip('"')


def desktop_dirs():
    found = []
    try:
        found.append(os.path.expandvars(reg_value(HKCU, CV + r"\Explorer\User Shell Folders", "Desktop")))
    except OSError:
        found.append(os.path.expandvars(r"%USERPROFILE%\Desktop"))
    found.append(os.path.expandvars(r"%PUBLIC%\Desktop"))
    return found


def shortcuts_in(directories, recurse=False):
    """Every .lnk and .url under the directories, read without Windows' help."""
    rows = []
    for directory in directories:
        base = pathlib.Path(directory)
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*") if recurse else base.iterdir()):
            suffix = path.suffix.lower()
            if suffix not in (".lnk", ".url") or not path.is_file():
                continue
            row = {"file": str(path), "kind": suffix[1:]}
            try:
                if suffix == ".lnk":
                    row.update(parse_lnk(path.read_bytes()))
                else:
                    row.update(parse_url(path.read_text(encoding="utf-8", errors="replace")))
            except (ValueError, struct.error, OSError) as error:
                row["error"] = f"{type(error).__name__}: {error}"
            # Where the icon would come from: the named icon file, else the target itself.
            source = os.path.expandvars(row.get("icon_location") or row.get("iconfile") or row.get("target") or "")
            row["icon_source_kind"] = pathlib.Path(source).suffix.lower() if source and os.path.isfile(source) else ("missing" if source else "none")
            rows.append(row)
    return rows


def windows_reads_shortcuts(files):
    """What Windows itself says about the same .lnk files (WScript.Shell)."""
    if not files:
        return {}
    listing = pathlib.Path(os.environ.get("TEMP", ".")) / "inwin-lnk-list.txt"
    listing.write_text("\n".join(files), encoding="utf-8")
    rows = powershell(
        f"$sh = New-Object -ComObject WScript.Shell; @(Get-Content -LiteralPath '{listing}' -Encoding UTF8 | ForEach-Object "
        "{ $s = $sh.CreateShortcut($_); [pscustomobject]@{ file = $_; target = $s.TargetPath; arguments = $s.Arguments; "
        "icon = $s.IconLocation; dir = $s.WorkingDirectory } })" + JSON, timeout=180)
    if isinstance(rows, dict):
        rows = [rows]
    return {row["file"]: row for row in rows or []}


def same_path(a, b):
    def clean(text):
        return os.path.expandvars(text or "").rstrip("\\").lower()
    return clean(a) == clean(b)


def compare_with_windows(rows):
    """How many shortcuts the offline reader reads as Windows does."""
    links = [row for row in rows if row["kind"] == "lnk" and "error" not in row]
    truth = windows_reads_shortcuts([row["file"] for row in links])
    result = {"shortcuts": len(links), "windows_answered": len(truth), "target_same": 0, "arguments_same": 0, "working_dir_same": 0,
              "windows_has_no_target": 0, "reader_has_no_target": 0, "different": []}
    for row in links:
        theirs = truth.get(row["file"])
        if theirs is None:
            continue
        if not theirs.get("target"):
            result["windows_has_no_target"] += 1
        if "target" not in row:
            result["reader_has_no_target"] += 1
        if same_path(row.get("target"), theirs.get("target")):
            result["target_same"] += 1
        elif len(result["different"]) < 12:
            result["different"].append({"file": os.path.basename(row["file"]), "reader": row.get("target", row.get("target_kind")), "windows": theirs.get("target")})
        if (row.get("arguments") or "") == (theirs.get("arguments") or ""):
            result["arguments_same"] += 1
        if same_path(row.get("working_dir"), theirs.get("dir")):
            result["working_dir_same"] += 1
    return result


# ---------------------------------------------------------------- the desktop's icons


def desktop_list_view():
    """The list view that holds the desktop's icons, or None."""
    sys.path.insert(0, str(HERE.parent / "agent"))
    import win

    for window in win.top_windows(visible_only=False):
        if window["class"] not in ("Progman", "WorkerW"):
            continue
        for child in win.children(window["hwnd"]):
            if win.class_of(child) == "SysListView32":
                return child
    return None


def desktop_icons_live():
    """Each icon's name and rectangle on the screen, through UI Automation."""
    sys.path.insert(0, str(HERE.parent / "agent"))
    import uia

    view = desktop_list_view()
    if view is None:
        raise LookupError("no desktop list view: this session has no Explorer desktop")
    count = user32.SendMessageW(wt.HWND(view), 0x1004, 0, 0)  # LVM_GETITEMCOUNT
    read = uia.Automation().read(view)
    items = [{"name": e["name"], "screen": e.get("screen")} for e in read["elements"] if e["role"] == "list item"]
    return {"list_view_items": count, "icons": items, "milliseconds": read["milliseconds"]}


def layout_fingerprint():
    """The registry's record of the desktop's layout, as hashes: has Explorer written it down?"""
    found = {}
    try:
        with winreg.OpenKey(HKCU, DESKTOP_BAG) as key:
            index = 0
            while True:
                try:
                    name, value, _ = winreg.EnumValue(key, index)
                except OSError:
                    break
                index += 1
                if isinstance(value, bytes):
                    found[name] = {"bytes": len(value), "sha256": hashlib.sha256(value).hexdigest()[:16], "names_our_shortcuts": b"T\0U\0R\0 \0T\0e\0s\0t\0" in value}
    except OSError as error:
        found["error"] = str(error)
    return found


def taskband_order(blob):
    """The shortcut names inside Taskband\\Favorites, in the order stored."""
    text = blob.decode("utf-16-le", "ignore")
    names = re.findall(r"[^\x00-\x1f\\/:*?\"<>|]{1,80}\.lnk", text)
    ordered = []
    for name in names:
        if name not in ordered:
            ordered.append(name)
    packaged = sorted(set(re.findall(r"[A-Za-z0-9.]{3,60}_[a-z0-9]{13}![A-Za-z0-9.]{1,40}", text)))
    return {"bytes": len(blob), "shortcut_names_in_order": ordered, "packaged_application_ids": packaged}


def start_layout():
    """Windows' own export of the Start menu's pins: its shape, not its text."""
    text = None
    problems = []
    for extension in ("xml", "json"):  # Windows 10 insists on the first, Windows 11 writes the second
        target = pathlib.Path(os.environ.get("TEMP", ".")) / f"inwin-start-layout.{extension}"
        if target.exists():
            target.unlink()
        try:
            powershell(f"Export-StartLayout -Path '{target}'; 'done'" + JSON)
            text = target.read_text(encoding="utf-8-sig", errors="replace")
            break
        except (RuntimeError, OSError) as error:
            problems.append(f"{extension}: {str(error)[:120]}")
    if text is None:
        raise RuntimeError("; ".join(problems))
    result = {"bytes": len(text), "asked_for": extension}
    if text.lstrip().startswith("{"):
        data = json.loads(text)
        pins = data.get("pinnedList", [])
        result.update({"format": "json", "pins": len(pins), "kinds": sorted({key for pin in pins for key in pin})})
        result["pinned"] = pins[:40]
    else:
        result.update({"format": "xml", "desktop_tiles": text.count("DesktopApplicationTile"), "packaged_tiles": text.count("<start:Tile "), "groups": text.count("<start:Group")})
    return result


def start_files():
    local = pathlib.Path(os.path.expandvars(r"%LOCALAPPDATA%\Packages\Microsoft.Windows.StartMenuExperienceHost_cw5n1h2txyewy\LocalState"))
    found = {"local_state": local.is_dir(), "files": {}}
    if local.is_dir():
        for path in local.iterdir():
            if path.is_file():
                found["files"][path.name] = {"bytes": path.stat().st_size, "starts_with": path.read_bytes()[:8].hex()}
    try:
        cloud = reg_subkeys(HKCU, CV + r"\CloudStore\Store\Cache\DefaultAccount")
        found["cloud_store_keys_about_start"] = [name for name in cloud if "start" in name.lower()][:12]
    except OSError as error:
        found["cloud_store"] = str(error)
    return found


def mounted_devices():
    """Drive letters as Windows records them: which partition each letter means."""
    rows = []
    with winreg.OpenKey(HKLM, r"SYSTEM\MountedDevices") as key:
        index = 0
        while True:
            try:
                name, value, _ = winreg.EnumValue(key, index)
            except OSError:
                break
            index += 1
            if not name.startswith("\\DosDevices\\"):
                continue
            value = bytes(value)
            row = {"letter": name[-2:], "bytes": len(value)}
            if value.startswith(b"DMIO:ID:") and len(value) == 24:
                import uuid

                row.update({"kind": "gpt", "partition_guid": str(uuid.UUID(bytes_le=value[8:24]))})
            elif len(value) == 12:
                signature, offset = struct.unpack("<IQ", value)
                row.update({"kind": "mbr", "disk_signature": f"{signature:08x}", "partition_offset": offset})
            else:
                row.update({"kind": "other", "starts_with": value[:16].hex()})
            rows.append(row)
    return rows


def volume_labels():
    rows = []
    kernel32.GetLogicalDrives.restype = wt.DWORD
    mask = kernel32.GetLogicalDrives()
    for bit in range(26):
        if not mask >> bit & 1:
            continue
        root = f"{chr(65 + bit)}:\\"
        label = ctypes.create_unicode_buffer(261)
        system = ctypes.create_unicode_buffer(261)
        serial = wt.DWORD(0)
        ok = kernel32.GetVolumeInformationW(ctypes.c_wchar_p(root), label, 261, ctypes.byref(serial), None, None, system, 261)
        rows.append({"letter": root[:2], "type": kernel32.GetDriveTypeW(ctypes.c_wchar_p(root)), "label": label.value if ok else None,
                     "filesystem": system.value if ok else None, "serial": f"{serial.value:08x}" if ok else None})
    return rows


def displays():
    rows = []
    proc = ctypes.WINFUNCTYPE(wt.BOOL, ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(wt.RECT), wt.LPARAM)

    class Info(ctypes.Structure):
        _fields_ = [("size", wt.DWORD), ("monitor", wt.RECT), ("work", wt.RECT), ("flags", wt.DWORD), ("device", wt.WCHAR * 32)]

    def each(monitor, _dc, _rect, _data):
        info = Info()
        info.size = ctypes.sizeof(Info)
        user32.GetMonitorInfoW(ctypes.c_void_p(monitor), ctypes.byref(info))
        x, y = wt.UINT(0), wt.UINT(0)
        try:
            ctypes.windll.shcore.GetDpiForMonitor(ctypes.c_void_p(monitor), 0, ctypes.byref(x), ctypes.byref(y))
        except (AttributeError, OSError):
            pass
        r = info.monitor
        rows.append({"device": info.device, "primary": bool(info.flags & 1), "rect": [r.left, r.top, r.right - r.left, r.bottom - r.top],
                     "work": [info.work.left, info.work.top, info.work.right - info.work.left, info.work.bottom - info.work.top], "dpi": x.value})
        return True

    callback = proc(each)
    user32.EnumDisplayMonitors(None, None, callback, 0)
    return rows


def display_registry():
    base = r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers\Configuration"
    names = reg_subkeys(HKLM, base)
    result = {"configurations": len(names)}
    if names:
        result["first"] = {"values": reg_values(HKLM, base + "\\" + names[0])}
        try:
            result["first"]["source_00"] = reg_values(HKLM, base + "\\" + names[0] + r"\00")
        except OSError:
            pass
    return result


def associations():
    shlwapi = ctypes.WinDLL("shlwapi")
    rows = {}
    for name in EXTENSIONS + PROTOCOLS:
        row = {}
        is_extension = name.startswith(".")
        choice = (CV + rf"\Explorer\FileExts\{name}\UserChoice") if is_extension else rf"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\{name}\UserChoice"
        try:
            row["user_choice"] = reg_value(HKCU, choice, "ProgId")
        except OSError:
            row["user_choice"] = None
        try:
            row["class_default"] = reg_value(HKCR, name, "")
        except OSError:
            row["class_default"] = None
        for label, what in (("executable", 2), ("friendly_name", 4)):
            size = wt.DWORD(1024)
            buffer = ctypes.create_unicode_buffer(1024)
            result = shlwapi.AssocQueryStringW(0, what, ctypes.c_wchar_p(name), None, buffer, ctypes.byref(size))
            row[label] = buffer.value if result == 0 else None
        rows[name] = row
    return rows


def installed_applications():
    result = {}
    for label, root, path in (
        ("machine", HKLM, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        ("machine-32-bit", HKLM, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        ("user", HKCU, r"Software\Microsoft\Windows\CurrentVersion\Uninstall"),
    ):
        try:
            keys = reg_subkeys(root, path)
        except OSError:
            result[label] = None
            continue
        counts = {"entries": len(keys), "with_name": 0, "with_icon": 0, "with_location": 0, "hidden_components": 0}
        for key in keys:
            try:
                values = reg_values(root, path + "\\" + key)
            except OSError:
                continue
            counts["with_name"] += bool(values.get("DisplayName"))
            counts["with_icon"] += bool(values.get("DisplayIcon"))
            counts["with_location"] += bool(values.get("InstallLocation"))
            counts["hidden_components"] += values.get("SystemComponent") == 1
        result[label] = counts
    try:
        result["app_paths"] = len(reg_subkeys(HKLM, r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths"))
    except OSError:
        result["app_paths"] = None
    return result


def offline_copy(out_dir):
    """Save this user's registry to a file, load the file as a stranger's
    registry would be loaded, and compare: is what the running Windows
    says what the file on the disk holds?"""
    hive = pathlib.Path(out_dir) / "inwin-ntuser-copy.dat"
    if hive.exists():
        hive.unlink()
    code, text = run_tool(["reg.exe", "save", "HKCU", str(hive), "/y"])
    if code != 0:
        raise PermissionError(f"reg save: exit {code}")
    result = {"file_bytes": hive.stat().st_size, "starts_with": hive.read_bytes()[:4].decode("ascii", "replace"), "compared": 0, "same": 0, "different": []}
    # First with a reader of the file format that asks Windows nothing (regf.py).
    try:
        from regf import Hive

        parsed = Hive(hive.read_bytes())
        own = {"compared": 0, "same": 0, "different": [], "keys_walked": 0, "values_walked": 0}
        for path, name in OFFLINE:
            try:
                with winreg.OpenKey(HKCU, path) as key:
                    live = winreg.QueryValueEx(key, name)[0]
            except OSError:
                continue
            own["compared"] += 1
            try:
                saved = parsed.value(path, name)
            except (KeyError, ValueError, struct.error) as error:
                saved = f"{type(error).__name__}"
            if saved == (bytes(live) if isinstance(live, bytes) else live):
                own["same"] += 1
            else:
                own["different"].append(f"{path}\\{name}")
        # And every value of whole keys, to exercise the reader on more than fourteen values.
        for path in (r"Control Panel\Desktop", r"Control Panel\International", CV + r"\Explorer\User Shell Folders", CV + r"\Explorer\Advanced", r"Environment", r"Control Panel\Colors"):
            try:
                with winreg.OpenKey(HKCU, path) as key:
                    index = 0
                    live_values = {}
                    while True:
                        try:
                            value_name, value, _ = winreg.EnumValue(key, index)
                        except OSError:
                            break
                        live_values[value_name] = bytes(value) if isinstance(value, bytes) else value
                        index += 1
                saved_values = {value_name: value for value_name, (_, value) in parsed.values(path).items()}
            except (OSError, KeyError, ValueError, struct.error) as error:
                own["different"].append(f"{path}: {type(error).__name__}")
                continue
            own["keys_walked"] += 1
            for value_name, value in live_values.items():
                own["values_walked"] += 1
                if saved_values.get(value_name) == value or (value is None and not saved_values.get(value_name)):
                    own["values_same"] = own.get("values_same", 0) + 1
                elif len(own["different"]) < 12:
                    own["different"].append(f"{path}\\{value_name}")
        result["own_reader"] = own
    except Exception as error:
        result["own_reader"] = {"error": f"{type(error).__name__}: {str(error)[:200]}"}
    code, text = run_tool(["reg.exe", "load", r"HKU\inwin_offline", str(hive)])
    if code != 0:
        raise PermissionError(f"reg load: exit {code}")
    try:
        for path, name in OFFLINE:
            try:
                live = reg_bytes(HKCU, path, name) if name in ("Favorites", "IconLayouts") else reg_value(HKCU, path, name)
            except OSError:
                continue
            result["compared"] += 1
            try:
                saved = reg_bytes(HKU, "inwin_offline\\" + path, name) if name in ("Favorites", "IconLayouts") else reg_value(HKU, "inwin_offline\\" + path, name)
            except OSError:
                saved = None
            if saved == live:
                result["same"] += 1
            else:
                result["different"].append(f"{path}\\{name}")
    finally:
        run_tool(["reg.exe", "unload", r"HKU\inwin_offline"])
        try:
            hive.unlink()
        except OSError:
            pass
    return result


# ---------------------------------------------------------------- the fixture


def write_bmp(path, width=640, height=400):
    """A test wallpaper of TUR's own: a plain gradient."""
    row_bytes = (width * 3 + 3) & ~3
    rows = bytearray()
    for y in range(height):
        row = bytearray()
        for x in range(width):
            row += bytes((40 + 120 * x // width, 60 + 100 * y // height, 90))
        rows += row + b"\0" * (row_bytes - width * 3)
    header = struct.pack("<2sIHHI", b"BM", 54 + len(rows), 0, 0, 54) + struct.pack("<IiiHHIIiiII", 40, width, height, 1, 24, 0, len(rows), 2835, 2835, 0, 0)
    pathlib.Path(path).write_bytes(header + bytes(rows))


def fixture(directory):
    directory = pathlib.Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    notes = {}
    wallpaper = directory / "tur-test-wallpaper.bmp"
    write_bmp(wallpaper)
    with winreg.OpenKey(HKCU, r"Control Panel\Desktop", 0, winreg.KEY_SET_VALUE) as key:
        winreg.SetValueEx(key, "WallpaperStyle", 0, winreg.REG_SZ, "6")
        winreg.SetValueEx(key, "TileWallpaper", 0, winreg.REG_SZ, "0")
    notes["wallpaper_set"] = bool(user32.SystemParametersInfoW(20, 0, ctypes.c_wchar_p(str(wallpaper)), 3))
    notes["wallpaper"] = file_facts(wallpaper)
    desktop = pathlib.Path(desktop_dirs()[0])
    desktop.mkdir(parents=True, exist_ok=True)
    windir = os.environ.get("SystemRoot", r"C:\Windows")
    try:
        made = powershell(
            "$sh = New-Object -ComObject WScript.Shell; "
            f"$a = $sh.CreateShortcut('{desktop}\\TUR Test Notepad.lnk'); $a.TargetPath = '{windir}\\System32\\notepad.exe'; "
            f"$a.Arguments = 'inwin-argument.txt'; $a.WorkingDirectory = '{windir}'; $a.IconLocation = '{windir}\\System32\\shell32.dll,23'; $a.Save(); "
            f"$b = $sh.CreateShortcut('{desktop}\\TUR Test Folder.lnk'); $b.TargetPath = '{windir}'; $b.Save(); 'made'" + JSON)
    except Exception as error:
        made = f"not made: {type(error).__name__}: {str(error)[:200]}"
    (desktop / "TUR Test Site.url").write_text("[InternetShortcut]\nURL=https://example.org/\n", encoding="ascii")
    notes["shortcuts"] = made
    time.sleep(2)
    try:
        view = desktop_list_view()
        notes["desktop_list_view"] = bool(view)
        if view:
            user32.GetParent.restype = ctypes.c_void_p
            user32.GetParent.argtypes = [ctypes.c_void_p]
            shell_view = user32.GetParent(view)

            def refresh():
                # The desktop's own Refresh command, and the F5 key for good measure.
                user32.SendMessageTimeoutW(wt.HWND(shell_view), 0x0111, 0x7103, 0, 2, 5000, None)
                user32.PostMessageW(wt.HWND(view), 0x100, 0x74, 0)
                time.sleep(3)

            notes["layout_before"] = layout_fingerprint()
            refresh()
            before = desktop_icons_live()
            notes["icons_before"] = before
            wanted = {"TUR Test Notepad": (300, 200), "TUR Test Folder": (500, 300), "TUR Test Site": (700, 120)}
            moved = {}
            for index, icon in enumerate(before["icons"]):
                for name, (x, y) in wanted.items():
                    if icon["name"].startswith(name):
                        user32.SendMessageW(wt.HWND(view), 0x100F, index, (y << 16) | x)  # LVM_SETITEMPOSITION
                        moved[name] = {"index": index, "asked": [x, y]}
            time.sleep(1)
            notes["moved"] = moved
            notes["icons_after_move"] = desktop_icons_live()
            notes["layout_after_move"] = layout_fingerprint()
            refresh()  # does Explorer write the layout down when the desktop is refreshed?
            notes["icons_after_refresh"] = desktop_icons_live()
            notes["layout_after_refresh"] = layout_fingerprint()
    except Exception as error:  # the fixture's limits are a finding
        notes["desktop_icons_error"] = f"{type(error).__name__}: {str(error)[:300]}"
    (directory / "fixture.json").write_text(json.dumps(notes, indent=1), encoding="utf-8")
    print(json.dumps({key: value for key, value in notes.items() if not key.startswith("icons_")}, indent=1)[:3000])


# ---------------------------------------------------------------- the reader


def read_all(out):
    p = Probes()
    run = p.run
    sid = None
    try:
        sid = my_sid()
    except Exception:
        pass
    appdata = os.environ.get("APPDATA", "")

    run("rights", token_facts, "the process token")
    # Wallpaper and colours.
    run("wallpaper", lambda: {**{name: reg_value(HKCU, r"Control Panel\Desktop", name) for name in ("WallPaper", "WallpaperStyle", "TileWallpaper")},
                              "file": file_facts(reg_value(HKCU, r"Control Panel\Desktop", "WallPaper")),
                              "transcoded_copy": file_facts(appdata + r"\Microsoft\Windows\Themes\TranscodedWallpaper")},
        r"HKCU\Control Panel\Desktop: WallPaper, WallpaperStyle, TileWallpaper; %AppData%\Microsoft\Windows\Themes\TranscodedWallpaper")
    run("wallpaper-per-monitor", lambda: {name: value for name, value in reg_values(HKCU, r"Control Panel\Desktop").items() if name.startswith("TranscodedImageCache")},
        r"HKCU\Control Panel\Desktop: TranscodedImageCache, TranscodedImageCache_000...")
    run("wallpaper-kind", lambda: reg_values(HKCU, CV + r"\Explorer\Wallpapers"), rf"HKCU\{CV}\Explorer\Wallpapers: BackgroundType")
    run("slideshow", lambda: {"ini": file_facts(appdata + r"\Microsoft\Windows\Themes\slideshow.ini"), "settings": reg_values(HKCU, r"Control Panel\Personalization\Desktop Slideshow")},
        r"%AppData%\Microsoft\Windows\Themes\slideshow.ini; HKCU\Control Panel\Personalization\Desktop Slideshow")
    run("background-colour", lambda: reg_value(HKCU, r"Control Panel\Colors", "Background"), r"HKCU\Control Panel\Colors: Background")
    run("accent-colour", lambda: {"dwm": {name: reg_value(HKCU, r"Software\Microsoft\Windows\DWM", name) for name in ("AccentColor", "ColorizationColor", "ColorPrevalence")},
                                  "accent": reg_values(HKCU, CV + r"\Explorer\Accent")},
        rf"HKCU\Software\Microsoft\Windows\DWM: AccentColor (ABGR), ColorizationColor; HKCU\{CV}\Explorer\Accent: AccentPalette")
    run("light-dark", lambda: reg_values(HKCU, CV + r"\Themes\Personalize"), rf"HKCU\{CV}\Themes\Personalize: AppsUseLightTheme, SystemUsesLightTheme")
    run("theme-file", lambda: reg_value(HKCU, CV + r"\Themes", "CurrentTheme"), rf"HKCU\{CV}\Themes: CurrentTheme")
    run("lock-screen (user settings)", lambda: reg_values(HKCU, CV + r"\Lock Screen"), rf"HKCU\{CV}\Lock Screen")
    run("lock-screen (machine record)", lambda: reg_values(HKLM, rf"SOFTWARE\Microsoft\Windows\CurrentVersion\SystemProtectedUserData\{sid}\AnyoneRead\LockScreen"),
        r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\SystemProtectedUserData\<SID>\AnyoneRead\LockScreen")
    run("lock-screen (picture files)", lambda: sorted(os.listdir(rf"C:\ProgramData\Microsoft\Windows\SystemData\{sid}\ReadOnly"))[:10],
        r"C:\ProgramData\Microsoft\Windows\SystemData\<SID>\ReadOnly\LockScreen_*")
    # The desktop.
    desktop = run("desktop-shortcuts", lambda: shortcuts_in(desktop_dirs()), "the Desktop folder and Public\\Desktop: .lnk ([MS-SHLLINK]) and .url (text)")
    run("desktop-shortcuts-as-windows-reads-them", lambda: compare_with_windows(desktop or []), "WScript.Shell on the same files")
    run("desktop-icons-live", desktop_icons_live, "the desktop's list view, through UI Automation")
    run("desktop-icon-layout", lambda: reg_values(HKCU, DESKTOP_BAG), rf"HKCU\{DESKTOP_BAG}: IconLayouts, ItemPos<width>x<height>x<dpi>(<n>), FFlags, IconSize")
    run("desktop-system-icons", lambda: reg_values(HKCU, CV + r"\Explorer\HideDesktopIcons\NewStartPanel"), rf"HKCU\{CV}\Explorer\HideDesktopIcons\NewStartPanel")
    # Taskbar and Start.
    pinned = appdata + r"\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar"
    run("taskbar-pinned-shortcuts", lambda: shortcuts_in([pinned]), r"%AppData%\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\*.lnk")
    run("taskbar-order", lambda: taskband_order(reg_bytes(HKCU, CV + r"\Explorer\Taskband", "Favorites")), rf"HKCU\{CV}\Explorer\Taskband: Favorites")
    run("start-pins (Windows' export)", start_layout, "Export-StartLayout")
    run("start-pins (files)", start_files, r"%LocalAppData%\Packages\Microsoft.Windows.StartMenuExperienceHost_cw5n1h2txyewy\LocalState\start*.bin; HKCU\...\CloudStore")
    began = time.perf_counter()
    menu = run("start-menu-shortcuts", lambda: shortcuts_in([os.path.expandvars(r"%ProgramData%\Microsoft\Windows\Start Menu\Programs"), appdata + r"\Microsoft\Windows\Start Menu\Programs"], recurse=True),
               r"%ProgramData%\Microsoft\Windows\Start Menu\Programs and %AppData%\...\Start Menu\Programs")
    run("start-menu-shortcuts-as-windows-reads-them", lambda: {**compare_with_windows(menu or []), "reader_ms": round((time.perf_counter() - began) * 1000)}, "WScript.Shell on the same files")
    run("start-apps", lambda: (lambda rows: {"entries": len(rows), "packaged": sum(1 for row in rows if "!" in (row.get("AppID") or "")), "sample_ids": [row.get("AppID") for row in rows[:8]]})(powershell("@(Get-StartApps)" + JSON) or []),
        "Get-StartApps (shell:AppsFolder)")
    run("packaged-applications", lambda: powershell("@(Get-AppxPackage).Count" + JSON), "Get-AppxPackage; on disk: Program Files\\WindowsApps\\*\\AppxManifest.xml")
    # Drives.
    run("drive-letters", mounted_devices, r"HKLM\SYSTEM\MountedDevices: \DosDevices\X:")
    run("volume-labels", volume_labels, "the label is in the file system itself; GetVolumeInformationW")
    run("partitions", lambda: powershell("Get-Partition | Select-Object DiskNumber,PartitionNumber,DriveLetter,Guid,GptType,Type,Size,IsHidden,NoDefaultDriveLetter" + JSON), "Get-Partition")
    run("drive-label-overrides", lambda: reg_subkeys(HKLM, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\DriveIcons"), r"HKLM\SOFTWARE\...\Explorer\DriveIcons\<letter>\DefaultLabel")
    run("hidden-drives-policy", lambda: reg_value(HKCU, CV + r"\Policies\Explorer", "NoDrives"), rf"HKCU\{CV}\Policies\Explorer: NoDrives")
    # Folders, associations, applications.
    run("known-folders (registry)", lambda: reg_values(HKCU, CV + r"\Explorer\User Shell Folders"), rf"HKCU\{CV}\Explorer\User Shell Folders")
    run("known-folders (Windows)", lambda: powershell("[pscustomobject]@{ Desktop=[Environment]::GetFolderPath('Desktop'); Documents=[Environment]::GetFolderPath('MyDocuments'); Pictures=[Environment]::GetFolderPath('MyPictures'); "
                                                      "Music=[Environment]::GetFolderPath('MyMusic'); Videos=[Environment]::GetFolderPath('MyVideos'); Downloads=(New-Object -ComObject Shell.Application).NameSpace('shell:Downloads').Self.Path }" + JSON), "the shell")
    run("onedrive", lambda: {"environment": os.environ.get("OneDrive"), "account": reg_values(HKCU, r"Software\Microsoft\OneDrive\Accounts\Personal").get("UserFolder")}, r"HKCU\Software\Microsoft\OneDrive\Accounts\Personal: UserFolder")
    run("associations", associations, rf"HKCU\{CV}\Explorer\FileExts\<.ext>\UserChoice: ProgId; HKCU\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\<protocol>\UserChoice; HKCR")
    run("installed-applications", installed_applications, r"HKLM and HKCU ...\CurrentVersion\Uninstall\*; HKLM\...\App Paths")
    # The account.
    logon = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\LogonUI"
    run("account-name", lambda: {"user": os.environ.get("USERNAME"), "sid_known": bool(sid), **{name: reg_value(HKLM, logon, name) for name in ("LastLoggedOnUser", "LastLoggedOnDisplayName")}}, rf"HKLM\{logon}: LastLoggedOnDisplayName")
    run("account-kind", lambda: powershell("Get-LocalUser -Name $env:USERNAME | Select-Object @{n='source';e={\"$($_.PrincipalSource)\"}},Enabled,@{n='has_full_name';e={[bool]$_.FullName}}" + JSON), "Get-LocalUser: PrincipalSource")
    run("account-microsoft-identity", lambda: len(reg_subkeys(HKCU, r"Software\Microsoft\IdentityCRL\UserExtendedProperties")), r"HKCU\Software\Microsoft\IdentityCRL\UserExtendedProperties")
    run("account-picture (registry)", lambda: sorted(reg_values(HKLM, rf"SOFTWARE\Microsoft\Windows\CurrentVersion\AccountPicture\Users\{sid}")), r"HKLM\SOFTWARE\...\AccountPicture\Users\<SID>: Image32...Image1080")
    run("account-picture (files)", lambda: sorted(os.listdir(rf"C:\Users\Public\AccountPictures\{sid}"))[:10], r"C:\Users\Public\AccountPictures\<SID>")
    run("pin-in-use", lambda: len(reg_subkeys(HKLM, logon + rf"\NgcPin\Credentials\{sid}")) >= 0, rf"HKLM\{logon}\NgcPin\Credentials\<SID> (existence)")
    run("hello-store", lambda: len(os.listdir(r"C:\Windows\ServiceProfiles\LocalService\AppData\Local\Microsoft\Ngc")), r"C:\Windows\ServiceProfiles\LocalService\AppData\Local\Microsoft\Ngc (existence)")
    run("biometrics-enrolled", lambda: reg_values(HKLM, rf"SOFTWARE\Microsoft\Windows\CurrentVersion\WinBio\AccountInfo\{sid}"), r"HKLM\SOFTWARE\...\WinBio\AccountInfo\<SID>: EnrolledFactors")
    run("account-database (live)", lambda: len(reg_subkeys(HKLM, r"SAM\SAM\Domains\Account\Users")), r"HKLM\SAM (the file Windows\System32\config\SAM)")
    # Regional settings, keyboard, time, displays.
    run("regional", lambda: {name: reg_value(HKCU, r"Control Panel\International", name) for name in ("LocaleName", "sShortDate", "sTimeFormat", "sDecimal", "iMeasure", "iFirstDayOfWeek")}, r"HKCU\Control Panel\International")
    run("languages", lambda: reg_value(HKCU, r"Control Panel\International\User Profile", "Languages"), r"HKCU\Control Panel\International\User Profile: Languages")
    run("keyboard-layouts", lambda: {"preload": reg_values(HKCU, r"Keyboard Layout\Preload"), "substitutes": reg_values(HKCU, r"Keyboard Layout\Substitutes")}, r"HKCU\Keyboard Layout\Preload, Substitutes")
    run("time-zone", lambda: reg_value(HKLM, r"SYSTEM\CurrentControlSet\Control\TimeZoneInformation", "TimeZoneKeyName"), r"HKLM\SYSTEM\CurrentControlSet\Control\TimeZoneInformation: TimeZoneKeyName")
    run("displays (live)", displays, "EnumDisplayMonitors, GetDpiForMonitor")
    run("display-scaling (registry)", lambda: {"per_monitor": {name: reg_values(HKCU, r"Control Panel\Desktop\PerMonitorSettings" + "\\" + name) for name in reg_subkeys(HKCU, r"Control Panel\Desktop\PerMonitorSettings")},
                                               "applied_dpi": reg_value(HKCU, r"Control Panel\Desktop\WindowMetrics", "AppliedDPI")}, r"HKCU\Control Panel\Desktop\PerMonitorSettings\<monitor>: DpiValue; ...\WindowMetrics: AppliedDPI")
    run("display-layout (registry)", display_registry, r"HKLM\SYSTEM\CurrentControlSet\Control\GraphicsDrivers\Configuration")
    # The same facts from the file.
    run("registry-file-holds-the-same", lambda: offline_copy(pathlib.Path(out).parent), "reg save HKCU, reg load, compare")

    # The layout values in full, for working out their format (names of icons only).
    layout = {}
    try:
        with winreg.OpenKey(HKCU, DESKTOP_BAG) as key:
            index = 0
            while True:
                try:
                    name, value, _ = winreg.EnumValue(key, index)
                except OSError:
                    break
                index += 1
                if isinstance(value, bytes) and (name == "IconLayouts" or name.startswith("ItemPos")):
                    layout[name] = bytes(value)[:16384].hex()
    except OSError:
        pass
    pathlib.Path(out).write_text(json.dumps({"probes": p.rows, "layout_values_hex": layout}, indent=1), encoding="utf-8")
    print(f"{sum(1 for row in p.rows if row['ok'])} of {len(p.rows)} facts read; rights: {p.value('rights')}")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--fixture")
    parser.add_argument("--out")
    arguments = parser.parse_args()
    if arguments.fixture:
        fixture(arguments.fixture)
    if arguments.out:
        read_all(arguments.out)


if __name__ == "__main__":
    main()
