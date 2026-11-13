/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md).

   What the framework uses from wdmsec.lib, a WDK static library (a
   Microsoft binary): IoCreateDeviceSecure (mingw-w64's wdmsec.h maps it to
   WdmlibIoCreateDeviceSecure) and the SDDL string constant it passes.
   Written from Microsoft's documentation. A user-space driver host has no
   security descriptors on device objects: the device is created with
   IoCreateDevice and the SDDL string and class GUID are not applied. */
#include <ntddk.h>
#include <wdmsec.h>

const UNICODE_STRING SDDL_DEVOBJ_SYS_ALL_ADM_ALL = RTL_CONSTANT_STRING(L"D:P(A;;GA;;;SY)(A;;GA;;;BA)");

NTSTATUS
WdmlibIoCreateDeviceSecure(
    PDRIVER_OBJECT DriverObject,
    ULONG DeviceExtensionSize,
    PUNICODE_STRING DeviceName,
    DEVICE_TYPE DeviceType,
    ULONG DeviceCharacteristics,
    BOOLEAN Exclusive,
    PCUNICODE_STRING DefaultSDDLString,
    LPCGUID DeviceClassGuid,
    PDEVICE_OBJECT *DeviceObject
    )
{
    UNREFERENCED_PARAMETER(DefaultSDDLString);
    UNREFERENCED_PARAMETER(DeviceClassGuid);
    return IoCreateDevice(DriverObject, DeviceExtensionSize, DeviceName, DeviceType, DeviceCharacteristics,
                          Exclusive, DeviceObject);
}
