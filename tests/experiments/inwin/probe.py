# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""What the read-only readers of the inwin experiment share
(docs/reports/exp-inwin-1.md): a list of named probes, each of which
returns a value or fails and is recorded either way; the rights the process
runs with; the registry; and one PowerShell call that answers in JSON.

Every probe only reads. Nothing here changes Windows.
"""

import ctypes
import ctypes.wintypes as wt
import json
import struct
import subprocess
import time
import winreg

advapi32 = ctypes.WinDLL("advapi32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

HKCU, HKLM, HKU, HKCR = winreg.HKEY_CURRENT_USER, winreg.HKEY_LOCAL_MACHINE, winreg.HKEY_USERS, winreg.HKEY_CLASSES_ROOT


class Probes:
    """Named reads. `where` says where the fact lives, for the report."""

    def __init__(self):
        self.rows = []

    def run(self, name, function, where=""):
        began = time.perf_counter()
        row = {"name": name, "where": where}
        try:
            row["value"] = function()
            row["ok"] = True
        except Exception as error:  # a probe that fails is a finding, not a crash
            row["ok"] = False
            row["error"] = f"{type(error).__name__}: {str(error)[:240]}"
        row["ms"] = round((time.perf_counter() - began) * 1000, 1)
        self.rows.append(row)
        return row.get("value")

    def value(self, name):
        for row in self.rows:
            if row["name"] == name:
                return row.get("value")
        return None


def powershell(script, timeout=90):
    """Run a read-only PowerShell expression and return what it wrote as JSON."""
    command = ("$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'; "
               "try { [Console]::OutputEncoding = New-Object Text.UTF8Encoding $false } catch {}; " + script)
    done = subprocess.run(
        ["powershell.exe", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", command],
        capture_output=True, text=True, timeout=timeout, encoding="utf-8", errors="replace",
    )
    out = done.stdout.strip().lstrip("\ufeff")
    if done.returncode != 0 or (not out and done.stderr.strip()):
        problem = " ".join(done.stderr.split())[:200] or f"exit {done.returncode}"
        raise RuntimeError(problem)
    if not out:
        return None
    try:
        return json.loads(out)
    except json.JSONDecodeError:
        return out[:400]


def run_tool(arguments, timeout=60):
    """A built-in tool's exit code and output (the caller keeps only facts, not text)."""
    done = subprocess.run(arguments, capture_output=True, text=True, timeout=timeout, errors="replace")
    return done.returncode, done.stdout + done.stderr


def reg_value(root, path, name):
    with winreg.OpenKey(root, path) as key:
        value, kind = winreg.QueryValueEx(key, name)
    if isinstance(value, bytes):
        return {"bytes": len(value), "hex": value[:64].hex()}
    return value


def reg_bytes(root, path, name):
    with winreg.OpenKey(root, path) as key:
        value, _ = winreg.QueryValueEx(key, name)
    return bytes(value)


def reg_values(root, path):
    """Every value of a key: name -> value (binary as length and first bytes)."""
    found = {}
    with winreg.OpenKey(root, path) as key:
        index = 0
        while True:
            try:
                name, value, _ = winreg.EnumValue(key, index)
            except OSError:
                break
            found[name] = {"bytes": len(value), "hex": bytes(value)[:48].hex()} if isinstance(value, bytes) else value
            index += 1
    return found


def reg_subkeys(root, path):
    names = []
    with winreg.OpenKey(root, path) as key:
        index = 0
        while True:
            try:
                names.append(winreg.EnumKey(key, index))
            except OSError:
                break
            index += 1
    return names


def token_facts():
    """The rights this process really has."""
    advapi32.OpenProcessToken.argtypes = [wt.HANDLE, wt.DWORD, ctypes.POINTER(wt.HANDLE)]
    advapi32.GetTokenInformation.argtypes = [wt.HANDLE, ctypes.c_int, ctypes.c_void_p, wt.DWORD, ctypes.POINTER(wt.DWORD)]
    advapi32.GetSidSubAuthorityCount.restype = ctypes.POINTER(ctypes.c_ubyte)
    advapi32.GetSidSubAuthorityCount.argtypes = [ctypes.c_void_p]
    advapi32.GetSidSubAuthority.restype = ctypes.POINTER(wt.DWORD)
    advapi32.GetSidSubAuthority.argtypes = [ctypes.c_void_p, wt.DWORD]
    kernel32.GetCurrentProcess.restype = wt.HANDLE
    token = wt.HANDLE()
    if not advapi32.OpenProcessToken(kernel32.GetCurrentProcess(), 0x8, ctypes.byref(token)):
        raise OSError(ctypes.get_last_error(), "OpenProcessToken")

    def information(kind):
        buffer = ctypes.create_string_buffer(256)
        got = wt.DWORD(0)
        if not advapi32.GetTokenInformation(token, kind, buffer, 256, ctypes.byref(got)):
            raise OSError(ctypes.get_last_error(), f"GetTokenInformation {kind}")
        return buffer

    elevated = struct.unpack("I", information(20).raw[:4])[0]
    kind = struct.unpack("I", information(18).raw[:4])[0]
    label = information(25)
    sid = ctypes.cast(label, ctypes.POINTER(ctypes.c_void_p))[0]
    count = advapi32.GetSidSubAuthorityCount(sid)[0]
    rid = advapi32.GetSidSubAuthority(sid, count - 1)[0]
    levels = {0x1000: "low", 0x2000: "medium", 0x2100: "medium plus", 0x3000: "high", 0x4000: "system"}
    return {
        "elevated": bool(elevated),
        "elevation_type": {1: "default", 2: "full", 3: "limited"}.get(kind, kind),
        "integrity": levels.get(rid, hex(rid)),
        "member_of_administrators": bool(ctypes.windll.shell32.IsUserAnAdmin()),
    }


class StartupInfo(ctypes.Structure):
    _fields_ = [("cb", wt.DWORD), ("reserved", wt.LPWSTR), ("desktop", wt.LPWSTR), ("title", wt.LPWSTR)] + \
               [(name, wt.DWORD) for name in ("x", "y", "xs", "ys", "xc", "yc", "fill", "flags")] + \
               [("show", wt.WORD), ("reserved2", wt.WORD), ("reserved3", ctypes.c_void_p)] + \
               [(name, wt.HANDLE) for name in ("stdin", "stdout", "stderr")]


class ProcessInformation(ctypes.Structure):
    _fields_ = [("process", wt.HANDLE), ("thread", wt.HANDLE), ("pid", wt.DWORD), ("tid", wt.DWORD)]


class SidAndAttributes(ctypes.Structure):
    _fields_ = [("sid", ctypes.c_void_p), ("attributes", wt.DWORD)]


def start_at_medium_integrity(command_line, wait_ms=120000):
    """Start a command as this account but at medium integrity: the level
    of an ordinary program on a person's desktop, below a program that
    was given administrator rights. Returns None, or what went wrong."""
    advapi32.OpenProcessToken.argtypes = [wt.HANDLE, wt.DWORD, ctypes.POINTER(wt.HANDLE)]
    advapi32.DuplicateTokenEx.argtypes = [wt.HANDLE, wt.DWORD, ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.POINTER(wt.HANDLE)]
    advapi32.ConvertStringSidToSidW.argtypes = [wt.LPCWSTR, ctypes.POINTER(ctypes.c_void_p)]
    advapi32.GetLengthSid.argtypes = [ctypes.c_void_p]
    advapi32.GetLengthSid.restype = wt.DWORD
    advapi32.SetTokenInformation.argtypes = [wt.HANDLE, ctypes.c_int, ctypes.c_void_p, wt.DWORD]
    advapi32.CreateProcessAsUserW.argtypes = [wt.HANDLE, wt.LPCWSTR, wt.LPWSTR, ctypes.c_void_p, ctypes.c_void_p, wt.BOOL, wt.DWORD,
                                              ctypes.c_void_p, wt.LPCWSTR, ctypes.c_void_p, ctypes.c_void_p]
    kernel32.GetCurrentProcess.restype = wt.HANDLE
    kernel32.WaitForSingleObject.argtypes = [wt.HANDLE, wt.DWORD]
    token, lowered = wt.HANDLE(), wt.HANDLE()
    if not advapi32.OpenProcessToken(kernel32.GetCurrentProcess(), 0x0001 | 0x0002 | 0x0008 | 0x0080 | 0x0100, ctypes.byref(token)):
        return f"OpenProcessToken: error {ctypes.get_last_error()}"
    if not advapi32.DuplicateTokenEx(token, 0x02000000, None, 2, 1, ctypes.byref(lowered)):
        return f"DuplicateTokenEx: error {ctypes.get_last_error()}"
    sid = ctypes.c_void_p()
    if not advapi32.ConvertStringSidToSidW("S-1-16-8192", ctypes.byref(sid)):
        return f"ConvertStringSidToSid: error {ctypes.get_last_error()}"
    label = SidAndAttributes(sid, 0x20)
    if not advapi32.SetTokenInformation(lowered, 25, ctypes.byref(label), ctypes.sizeof(label) + advapi32.GetLengthSid(sid)):
        return f"SetTokenInformation: error {ctypes.get_last_error()}"
    startup = StartupInfo()
    startup.cb = ctypes.sizeof(StartupInfo)
    information = ProcessInformation()
    buffer = ctypes.create_unicode_buffer(command_line)
    if not advapi32.CreateProcessAsUserW(lowered, None, buffer, None, None, False, 0x08000000, None, None, ctypes.byref(startup), ctypes.byref(information)):
        return f"CreateProcessAsUser: error {ctypes.get_last_error()}"
    kernel32.WaitForSingleObject(information.process, wait_ms)
    return None
