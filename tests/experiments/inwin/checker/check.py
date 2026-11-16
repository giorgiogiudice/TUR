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
import struct
import sys
import uuid

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))
from probe import HKLM, Probes, device_control, kernel32, powershell, reg_value, run_tool, token_facts  # noqa: E402

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


GPT_KINDS = {
    "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7": "basic data", "c12a7328-f81f-11d2-ba4b-00a0c93ec93b": "efi system",
    "e3c9e316-0b5c-4db8-817d-f92df00215ae": "microsoft reserved", "de94bba4-06d1-4d40-a16a-bfd50179d6ac": "windows recovery",
    "0fc63daf-8483-4772-8e79-3d69d8477de4": "linux data", "4f68bce3-e8cd-4db1-96e7-fbcaf984b709": "linux root (x86-64)",
    "b921b045-1df0-41c3-af44-4c6f280d3fae": "linux root (arm64)",
}


def system_volume_extents():
    """Which disk the Windows volume is on, and where (no rights needed)."""
    raw = device_control(r"\\.\%s" % os.environ.get("SystemDrive", "C:"), 0x00560000, 1024)
    count = struct.unpack_from("<I", raw, 0)[0]
    return [dict(zip(("disk", "offset", "length"), struct.unpack_from("<I4xqq", raw, 8 + 24 * i))) for i in range(count)]


def drive_layout():
    """The partition table of the disk that holds Windows, asked of the disk
    opened with no access rights (IOCTL_DISK_GET_DRIVE_LAYOUT_EX)."""
    disk = system_volume_extents()[0]["disk"]
    raw = device_control(rf"\\.\PhysicalDrive{disk}", 0x00070050, 65536)
    style, count = struct.unpack_from("<II", raw, 0)
    result = {"disk": disk, "style": {0: "mbr", 1: "gpt", 2: "raw"}.get(style, style), "partitions": []}
    if style == 1:
        result["disk_guid"] = str(uuid.UUID(bytes_le=raw[8:24]))
        result["usable_offset"], result["usable_length"] = struct.unpack_from("<qq", raw, 24)
    elif style == 0:
        result["signature"] = f"{struct.unpack_from('<I', raw, 8)[0]:08x}"
    for i in range(count):
        base = 48 + 144 * i
        offset, length, number = struct.unpack_from("<qqI", raw, base + 8)
        if length == 0:
            continue
        row = {"number": number, "offset": offset, "length": length}
        if style == 1:
            kind = str(uuid.UUID(bytes_le=raw[base + 32:base + 48]))
            row.update({"type": GPT_KINDS.get(kind, kind), "guid": str(uuid.UUID(bytes_le=raw[base + 48:base + 64])),
                        "attributes": f"{struct.unpack_from('<Q', raw, base + 64)[0]:#x}"})
        else:
            row.update({"mbr_type": f"{raw[base + 32]:#04x}", "active": bool(raw[base + 33])})
        result["partitions"].append(row)
    return result


def volume_dirty():
    raw = device_control(r"\\.\%s" % os.environ.get("SystemDrive", "C:"), 0x00090078, 4)
    return {"dirty": bool(struct.unpack_from("<I", raw, 0)[0] & 1)}


def ntfs_volume_data():
    """Clusters free and where the master file table lies: the bound a
    shrink meets (FSCTL_GET_NTFS_VOLUME_DATA)."""
    raw = device_control(r"\\.\%s" % os.environ.get("SystemDrive", "C:"), 0x00090064, 256)
    (_serial, _sectors, total, free, _reserved, sector, cluster, _record, _per, _valid, mft, _mft2, zone_start, zone_end) = struct.unpack_from("<qqqqqIIIIqqqqq", raw, 0)
    return {"cluster_bytes": cluster, "total_gib": round(total * cluster / 2**30, 1), "free_gib": round(free * cluster / 2**30, 1),
            "mft_starts_at_gib": round(mft * cluster / 2**30, 2), "mft_zone_ends_at_gib": round(zone_end * cluster / 2**30, 2), "sector_bytes": sector,
            "zone_start_gib": round(zone_start * cluster / 2**30, 2)}


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
    run("partitions (WMI)", lambda: powershell("Get-CimInstance Win32_DiskPartition | Select-Object DiskIndex,Index,Type,Size,StartingOffset,Bootable,PrimaryPartition" + JSON), "Win32_DiskPartition")
    run("volume-on-disk (IOCTL)", system_volume_extents, "IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS on the volume, opened with no access")
    run("partitions (IOCTL)", drive_layout, "IOCTL_DISK_GET_DRIVE_LAYOUT_EX on the disk, opened with no access")
    # Windows' own estimate of what may be taken from the volume took 51 s on one runner and over 90 s on another (run 43552339780).
    run("shrink-room", lambda: powershell("Get-PartitionSupportedSize -DriveLetter $env:SystemDrive[0] | Select-Object SizeMin,SizeMax" + JSON, timeout=240), "Get-PartitionSupportedSize")
    run("shrink-room (NTFS volume data)", ntfs_volume_data, "FSCTL_GET_NTFS_VOLUME_DATA on the volume, opened with no access")
    run("bitlocker (shell property)", lambda: powershell("(New-Object -ComObject Shell.Application).NameSpace($env:SystemDrive).Self.ExtendedProperty('System.Volume.BitLockerProtection')" + JSON), "System.Volume.BitLockerProtection")
    run("bitlocker (cmdlet)", lambda: powershell("Get-BitLockerVolume | Select-Object MountPoint,VolumeStatus,ProtectionStatus,EncryptionPercentage" + JSON), "Get-BitLockerVolume")
    run("bitlocker (manage-bde)", lambda: tool_says(["manage-bde.exe", "-status", os.environ.get("SystemDrive", "C:")], ["protection on", "protection off", "fully decrypted", "fully encrypted"]), "manage-bde -status")
    run("bitlocker (WMI)", lambda: powershell("Get-CimInstance -Namespace root/cimv2/Security/MicrosoftVolumeEncryption -ClassName Win32_EncryptableVolume | Select-Object DriveLetter,ProtectionStatus,ConversionStatus" + JSON), "Win32_EncryptableVolume")
    run("fast-startup (registry)", lambda: reg_value(HKLM, POWER, "HiberbootEnabled"), rf"HKLM\{POWER}")
    run("hibernation (registry)", lambda: reg_value(HKLM, r"SYSTEM\CurrentControlSet\Control\Power", "HibernateEnabled"), r"HKLM\SYSTEM\CurrentControlSet\Control\Power")
    run("hibernation-file", lambda: os.path.exists(os.environ.get("SystemDrive", "C:") + "\\hiberfil.sys"), "the file itself")
    run("sleep-states (powercfg)", lambda: tool_says(["powercfg.exe", "/a"], ["hibernate", "fast startup"]), "powercfg /a")
    run("volume-dirty (fsutil)", lambda: tool_says(["fsutil.exe", "dirty", "query", os.environ.get("SystemDrive", "C:")], ["not dirty", "is dirty"]), "fsutil dirty query")
    run("volume-dirty (FSCTL)", volume_dirty, "FSCTL_IS_VOLUME_DIRTY on the volume, opened with no access")
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
    if space and space.get("free_gib", 0) < 41:
        # The installer's own limits (pull requests 112 and 116): 25 GiB for TUR, 16 GiB left free for Windows.
        advice.append(f"Only {space.get('free_gib')} GiB are free on the Windows drive: TUR needs 25 GiB of it and leaves Windows 16 GiB free.")
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
