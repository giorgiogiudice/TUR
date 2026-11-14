#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The "get TUR" checker, as a prototype that only reads and reports
(docs/reports/exp-inwin-1.md, section 6): what a small Windows program can
learn about a PC before TUR is installed beside its Windows, and with which
rights. It changes nothing: no setting, no disk, no boot entry.

    check.py --out FILE.json

Each fact is read by more than one route where one of them needs
administrator rights, so that the run shows which route a program without
them can use.
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import json
import os
import pathlib
import platform
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))
from probe import HKLM, Probes, kernel32, powershell, reg_value, run_tool, token_facts  # noqa: E402

JSON = " | ConvertTo-Json -Depth 4 -Compress"
POWER = r"SYSTEM\CurrentControlSet\Control\Session Manager\Power"
NT = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"


def memory():
    class Status(ctypes.Structure):
        _fields_ = [("length", wt.DWORD), ("load", wt.DWORD)] + [(name, ctypes.c_uint64) for name in ("total", "free", "page_total", "page_free", "virtual_total", "virtual_free", "extended")]

    status = Status()
    status.length = ctypes.sizeof(Status)
    if not kernel32.GlobalMemoryStatusEx(ctypes.byref(status)):
        raise OSError("GlobalMemoryStatusEx")
    return {"total_mib": status.total >> 20, "free_mib": status.free >> 20}


def firmware_type():
    kind = wt.DWORD(0)
    if not kernel32.GetFirmwareType(ctypes.byref(kind)):
        raise OSError("GetFirmwareType")
    return {1: "bios", 2: "uefi"}.get(kind.value, f"unknown {kind.value}")


def free_space():
    free, total = ctypes.c_uint64(0), ctypes.c_uint64(0)
    drive = os.environ.get("SystemDrive", "C:") + "\\"
    if not kernel32.GetDiskFreeSpaceExW(ctypes.c_wchar_p(drive), None, ctypes.byref(total), ctypes.byref(free)):
        raise OSError("GetDiskFreeSpaceExW")
    return {"drive": drive, "total_gib": round(total.value / 2**30, 1), "free_gib": round(free.value / 2**30, 1)}


def power():
    class Status(ctypes.Structure):
        _fields_ = [("ac", ctypes.c_ubyte), ("battery_flag", ctypes.c_ubyte), ("percent", ctypes.c_ubyte), ("saver", ctypes.c_ubyte), ("life", wt.DWORD), ("full", wt.DWORD)]

    status = Status()
    if not kernel32.GetSystemPowerStatus(ctypes.byref(status)):
        raise OSError("GetSystemPowerStatus")
    return {"on_mains": status.ac == 1, "has_battery": not (status.battery_flag & 128), "percent": status.percent}


def tool_says(arguments, words):
    """Which of `words` a tool's output contains: facts, not its text."""
    code, text = run_tool(arguments)
    low = text.lower()
    if code != 0 and ("denied" in low or "elevat" in low or "administrator" in low or "privilege" in low):
        raise PermissionError(f"exit {code}: needs administrator rights")
    if code != 0:
        raise RuntimeError(f"exit {code}")
    return {word: (word in low) for word in words}


def pending_restart():
    import winreg

    found = {}
    for label, path in (
        ("servicing", r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending"),
        ("update", r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired"),
    ):
        try:
            winreg.CloseKey(winreg.OpenKey(HKLM, path))
            found[label] = True
        except FileNotFoundError:
            found[label] = False
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", required=True)
    arguments = parser.parse_args()
    p = Probes()
    run = p.run

    run("rights", token_facts, "the process token")
    run("windows", lambda: {name: reg_value(HKLM, NT, name) for name in ("ProductName", "DisplayVersion", "CurrentBuild", "EditionID", "InstallationType")}, rf"HKLM\{NT}")
    run("architecture", lambda: {"machine": platform.machine(), "processor": os.environ.get("PROCESSOR_ARCHITECTURE")}, "environment")
    run("memory", memory, "GlobalMemoryStatusEx")
    run("firmware-type", firmware_type, "GetFirmwareType")
    run("secure-boot (registry)", lambda: reg_value(HKLM, r"SYSTEM\CurrentControlSet\Control\SecureBoot\State", "UEFISecureBootEnabled"), r"HKLM\SYSTEM\CurrentControlSet\Control\SecureBoot\State")
    run("secure-boot (cmdlet)", lambda: powershell("Confirm-SecureBootUEFI" + JSON), "Confirm-SecureBootUEFI")
    run("virtualisation (processor feature)", lambda: {"firmware_enabled": bool(kernel32.IsProcessorFeaturePresent(21)), "second_level_translation": bool(kernel32.IsProcessorFeaturePresent(20))}, "IsProcessorFeaturePresent 21, 20")
    run("virtualisation (CIM)", lambda: powershell("Get-CimInstance Win32_Processor | Select-Object -First 1 VirtualizationFirmwareEnabled,VMMonitorModeExtensions,SecondLevelAddressTranslationExtensions,NumberOfCores,NumberOfLogicalProcessors" + JSON), "Win32_Processor")
    run("hypervisor-present", lambda: powershell("(Get-CimInstance Win32_ComputerSystem).HypervisorPresent" + JSON), "Win32_ComputerSystem")
    run("system-drive-space", free_space, "GetDiskFreeSpaceExW")
    run("volumes", lambda: powershell("Get-Volume | Select-Object DriveLetter,FileSystem,DriveType,HealthStatus,Size,SizeRemaining" + JSON), "Get-Volume (Storage module)")
    run("disks", lambda: powershell("Get-Disk | Select-Object Number,BusType,PartitionStyle,Size,LogicalSectorSize,PhysicalSectorSize,IsBoot,IsSystem,NumberOfPartitions,LargestFreeExtent" + JSON), "Get-Disk")
    run("partitions", lambda: powershell("Get-Partition | Select-Object DiskNumber,PartitionNumber,Type,GptType,Size,DriveLetter,IsBoot,IsSystem,IsHidden,NoDefaultDriveLetter" + JSON), "Get-Partition")
    run("shrink-room", lambda: powershell("Get-PartitionSupportedSize -DriveLetter $env:SystemDrive[0] | Select-Object SizeMin,SizeMax" + JSON), "Get-PartitionSupportedSize")
    run("bitlocker (shell property)", lambda: powershell("(New-Object -ComObject Shell.Application).NameSpace($env:SystemDrive).Self.ExtendedProperty('System.Volume.BitLockerProtection')" + JSON), "System.Volume.BitLockerProtection")
    run("bitlocker (cmdlet)", lambda: powershell("Get-BitLockerVolume | Select-Object MountPoint,VolumeStatus,ProtectionStatus,EncryptionPercentage" + JSON), "Get-BitLockerVolume")
    run("bitlocker (manage-bde)", lambda: tool_says(["manage-bde.exe", "-status", os.environ.get("SystemDrive", "C:")], ["protection on", "protection off", "fully decrypted", "fully encrypted"]), "manage-bde -status")
    run("bitlocker (WMI)", lambda: powershell("Get-CimInstance -Namespace root/cimv2/Security/MicrosoftVolumeEncryption -ClassName Win32_EncryptableVolume | Select-Object DriveLetter,ProtectionStatus,ConversionStatus" + JSON), "Win32_EncryptableVolume")
    run("fast-startup (registry)", lambda: reg_value(HKLM, POWER, "HiberbootEnabled"), rf"HKLM\{POWER}")
    run("hibernation (registry)", lambda: reg_value(HKLM, r"SYSTEM\CurrentControlSet\Control\Power", "HibernateEnabled"), r"HKLM\SYSTEM\CurrentControlSet\Control\Power")
    run("hibernation-file", lambda: os.path.exists(os.environ.get("SystemDrive", "C:") + "\\hiberfil.sys"), "the file itself")
    run("sleep-states (powercfg)", lambda: tool_says(["powercfg.exe", "/a"], ["hibernate", "fast startup"]), "powercfg /a")
    run("volume-dirty (fsutil)", lambda: tool_says(["fsutil.exe", "dirty", "query", os.environ.get("SystemDrive", "C:")], ["not dirty", "is dirty"]), "fsutil dirty query")
    run("recovery-environment (reagentc)", lambda: tool_says(["reagentc.exe", "/info"], ["enabled", "disabled"]), "reagentc /info")
    run("firmware-boot-entries (bcdedit)", lambda: tool_says(["bcdedit.exe", "/enum", "firmware"], ["{bootmgr}", "{fwbootmgr}"]), "bcdedit /enum firmware")
    run("tpm", lambda: powershell("Get-Tpm | Select-Object TpmPresent,TpmReady,TpmEnabled" + JSON), "Get-Tpm")
    run("power", power, "GetSystemPowerStatus")
    run("pending-restart", pending_restart, "registry keys under HKLM")
    run("storage-controllers", lambda: powershell("Get-CimInstance Win32_SCSIController | Select-Object Name,DriverName" + JSON), "Win32_SCSIController")
    run("usb-drives", lambda: powershell("@(Get-Disk | Where-Object BusType -eq 'USB').Count" + JSON), "Get-Disk")
    run("windows-licence-channel", lambda: powershell("Get-CimInstance SoftwareLicensingProduct -Filter \"PartialProductKey IS NOT NULL AND ApplicationID='55c92734-d682-4d71-983e-d6ec3f16059f'\" | Select-Object -First 1 LicenseStatus,ProductKeyChannel" + JSON), "SoftwareLicensingProduct (no key is read)")
    run("firmware-holds-a-windows-key", lambda: powershell("[bool]((Get-CimInstance SoftwareLicensingService).OA3xOriginalProductKey)" + JSON), "SoftwareLicensingService (presence only)")
    run("clock-is-utc", lambda: reg_value(HKLM, r"SYSTEM\CurrentControlSet\Control\TimeZoneInformation", "RealTimeIsUniversal"), r"HKLM\SYSTEM\...\TimeZoneInformation")
    run("device-encryption-ready", lambda: powershell("(Get-CimInstance -Namespace root/cimv2/Security/MicrosoftTpm -ClassName Win32_Tpm) -ne $null" + JSON), "Win32_Tpm")

    # What the checker would tell the person: computed only from what it could read.
    advice = []
    value = p.value
    if value("firmware-type") == "bios":
        advice.append("This PC starts in legacy BIOS mode: TUR installs beside Windows only on UEFI PCs.")
    space = value("system-drive-space") or {}
    if space and space.get("free_gib", 0) < 60:
        advice.append(f"Only {space.get('free_gib')} GiB are free on the Windows drive: TUR needs about 60 GiB of it.")
    if value("fast-startup (registry)") == 1:
        advice.append("Fast Startup is on: it must be switched off before another system touches this disk.")
    lock = value("bitlocker (shell property)")
    if lock in (1, 3, 5):
        advice.append("The Windows drive is encrypted with BitLocker: it must be suspended for the change, and the recovery key saved first.")
    memory_now = value("memory") or {}
    if memory_now and memory_now.get("total_mib", 0) < 7600:
        advice.append("Under 8 GiB of memory: Windows inside TUR will be slow.")
    if not advice:
        advice.append("Nothing this checker could read stands in the way.")

    result = {"probes": p.rows, "advice": advice}
    pathlib.Path(arguments.out).write_text(json.dumps(result, indent=1), encoding="utf-8")
    print(f"{sum(1 for row in p.rows if row['ok'])} of {len(p.rows)} facts read; rights: {p.value('rights')}")


if __name__ == "__main__":
    main()
