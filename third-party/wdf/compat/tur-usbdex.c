/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md).

   The USB client-contract functions the framework calls (USBD_CreateHandle,
   USBD_UrbAllocate, ...). On Windows they come from a WDK static library,
   usbdex.lib, which is a Microsoft binary; these are written for this build
   from Microsoft's documentation of each function:

   - a USBD handle is the client's registration with the USB stack; here it
     only records the devices and the tag;
   - URBs are allocated zeroed from non-paged pool (an isochronous URB with
     room for its packets) and freed with the handle that allocated them;
   - USBD_AssignUrbToIoStackLocation puts the URB in Argument1 of the
     internal device-control stack location, as the stack expects;
   - USBD_QueryUsbCapability reports every capability as not supported:
     Wine's USB bus driver (wineusb.sys) offers none of them. */
#include <ntddk.h>
#include <usb.h>
#include <usbdlib.h>

typedef struct _TUR_USBD_HANDLE {
    PDEVICE_OBJECT DeviceObject;
    PDEVICE_OBJECT TargetDeviceObject;
    ULONG ContractVersion;
    ULONG PoolTag;
} TUR_USBD_HANDLE;

NTSTATUS NTAPI USBD_CreateHandle(PDEVICE_OBJECT DeviceObject, PDEVICE_OBJECT TargetDeviceObject,
                                 ULONG USBDClientContractVersion, ULONG PoolTag, USBD_HANDLE *USBDHandle)
{
    TUR_USBD_HANDLE *handle;

    if (USBDHandle == NULL) {
        return STATUS_INVALID_PARAMETER;
    }
    *USBDHandle = NULL;
    if (USBDClientContractVersion != USBD_CLIENT_CONTRACT_VERSION_602) {
        return STATUS_INVALID_PARAMETER;
    }
    handle = (TUR_USBD_HANDLE *) ExAllocatePool2(POOL_FLAG_NON_PAGED, sizeof(*handle), PoolTag);
    if (handle == NULL) {
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    handle->DeviceObject = DeviceObject;
    handle->TargetDeviceObject = TargetDeviceObject;
    handle->ContractVersion = USBDClientContractVersion;
    handle->PoolTag = PoolTag;
    *USBDHandle = (USBD_HANDLE) handle;
    return STATUS_SUCCESS;
}

VOID NTAPI USBD_CloseHandle(USBD_HANDLE USBDHandle)
{
    if (USBDHandle != NULL) {
        ExFreePool(USBDHandle);
    }
}

NTSTATUS NTAPI USBD_QueryUsbCapability(USBD_HANDLE USBDHandle, const GUID *CapabilityType, ULONG OutputBufferLength,
                                       PUCHAR OutputBuffer, PULONG ResultLength)
{
    UNREFERENCED_PARAMETER(USBDHandle);
    UNREFERENCED_PARAMETER(CapabilityType);
    UNREFERENCED_PARAMETER(OutputBufferLength);
    UNREFERENCED_PARAMETER(OutputBuffer);
    if (ResultLength != NULL) {
        *ResultLength = 0;
    }
    return STATUS_NOT_SUPPORTED;
}

static ULONG tur_tag(USBD_HANDLE USBDHandle)
{
    return USBDHandle != NULL ? ((TUR_USBD_HANDLE *) USBDHandle)->PoolTag : 'bsUT';
}

NTSTATUS NTAPI USBD_UrbAllocate(USBD_HANDLE USBDHandle, PURB *Urb)
{
    if (Urb == NULL) {
        return STATUS_INVALID_PARAMETER;
    }
    *Urb = (PURB) ExAllocatePool2(POOL_FLAG_NON_PAGED, sizeof(URB), tur_tag(USBDHandle));
    return *Urb != NULL ? STATUS_SUCCESS : STATUS_INSUFFICIENT_RESOURCES;
}

NTSTATUS NTAPI USBD_IsochUrbAllocate(USBD_HANDLE USBDHandle, ULONG NumberOfIsochPacket, PURB *Urb)
{
    if (Urb == NULL || NumberOfIsochPacket == 0) {
        return STATUS_INVALID_PARAMETER;
    }
    *Urb = (PURB) ExAllocatePool2(POOL_FLAG_NON_PAGED, GET_ISO_URB_SIZE(NumberOfIsochPacket), tur_tag(USBDHandle));
    return *Urb != NULL ? STATUS_SUCCESS : STATUS_INSUFFICIENT_RESOURCES;
}

VOID NTAPI USBD_UrbFree(USBD_HANDLE USBDHandle, PURB Urb)
{
    UNREFERENCED_PARAMETER(USBDHandle);
    if (Urb != NULL) {
        ExFreePool(Urb);
    }
}

VOID NTAPI USBD_AssignUrbToIoStackLocation(USBD_HANDLE USBDHandle, PIO_STACK_LOCATION IoStackLocation, PURB Urb)
{
    UNREFERENCED_PARAMETER(USBDHandle);
    IoStackLocation->Parameters.Others.Argument1 = Urb;
}
