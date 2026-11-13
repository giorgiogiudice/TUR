/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md).

   Wraps mingw-w64's usbdlib.h, which predates the USB driver stack's
   "client contract" API of Windows 8 (USBD_HANDLE, USBD_CreateHandle, ...):
   includes it, then declares that API as Microsoft documents it. The
   functions live in a WDK static library (usbdex.lib) on Windows; this build
   implements them in tur-usbdex.c. The capability GUIDs and the IOCTL code
   only need to be distinct here: no USB bus driver in Wine answers a
   capability query, so their values are this file's own. */
#include_next <usbdlib.h>

#ifndef _TUR_USBDLIB_ADDITIONS_
#define _TUR_USBDLIB_ADDITIONS_

#ifdef __cplusplus
extern "C" {
#endif

#ifndef USBD_CLIENT_CONTRACT_VERSION_602
DECLARE_HANDLE(USBD_HANDLE);

#define USBD_CLIENT_CONTRACT_VERSION_INVALID 0xFFFFFFFF
#define USBD_CLIENT_CONTRACT_VERSION_602     0x602

NTSTATUS NTAPI USBD_CreateHandle(_In_ PDEVICE_OBJECT DeviceObject, _In_ PDEVICE_OBJECT TargetDeviceObject,
                                 _In_ ULONG USBDClientContractVersion, _In_ ULONG PoolTag,
                                 _Out_ USBD_HANDLE *USBDHandle);
VOID NTAPI USBD_CloseHandle(_In_ USBD_HANDLE USBDHandle);
NTSTATUS NTAPI USBD_QueryUsbCapability(_In_ USBD_HANDLE USBDHandle, _In_ const GUID *CapabilityType,
                                       _In_ ULONG OutputBufferLength, _Inout_opt_ PUCHAR OutputBuffer,
                                       _Out_opt_ PULONG ResultLength);
NTSTATUS NTAPI USBD_UrbAllocate(_In_ USBD_HANDLE USBDHandle, _Outptr_ PURB *Urb);
VOID NTAPI USBD_UrbFree(_In_ USBD_HANDLE USBDHandle, _In_ PURB Urb);
NTSTATUS NTAPI USBD_IsochUrbAllocate(_In_ USBD_HANDLE USBDHandle, _In_ ULONG NumberOfIsochPacket,
                                     _Outptr_ PURB *Urb);
VOID NTAPI USBD_AssignUrbToIoStackLocation(_In_ USBD_HANDLE USBDHandle, _In_ PIO_STACK_LOCATION IoStackLocation,
                                           _In_ PURB Urb);
#endif

#ifndef IOCTL_INTERNAL_USB_GET_USB_CAPABILITY
#define IOCTL_INTERNAL_USB_GET_USB_CAPABILITY CTL_CODE(FILE_DEVICE_USB, 0x7fe, METHOD_NEITHER, FILE_ANY_ACCESS)
#endif

#ifndef USBD_PF_HANDLES_SSP_HIGH_BANDWIDTH_ISOCH
#define USBD_PF_HANDLES_SSP_HIGH_BANDWIDTH_ISOCH 0x00000100
#endif
#ifndef USBD_PF_SSP_HIGH_BANDWIDTH_ISOCH
#define USBD_PF_SSP_HIGH_BANDWIDTH_ISOCH 0x00010000
#endif

#ifdef __cplusplus
}
#endif

DEFINE_GUID(GUID_USB_CAPABILITY_CHAINED_MDLS,
    0x7a5e4c01, 0x1d2b, 0x4f6a, 0x9a, 0x01, 0x54, 0x55, 0x52, 0x00, 0x00, 0x01);
DEFINE_GUID(GUID_USB_CAPABILITY_STATIC_STREAMS,
    0x7a5e4c01, 0x1d2b, 0x4f6a, 0x9a, 0x01, 0x54, 0x55, 0x52, 0x00, 0x00, 0x02);
DEFINE_GUID(GUID_USB_CAPABILITY_SELECTIVE_SUSPEND,
    0x7a5e4c01, 0x1d2b, 0x4f6a, 0x9a, 0x01, 0x54, 0x55, 0x52, 0x00, 0x00, 0x03);
DEFINE_GUID(GUID_USB_CAPABILITY_FUNCTION_SUSPEND,
    0x7a5e4c01, 0x1d2b, 0x4f6a, 0x9a, 0x01, 0x54, 0x55, 0x52, 0x00, 0x00, 0x04);
DEFINE_GUID(GUID_USB_CAPABILITY_DEVICE_CONNECTION_HIGH_SPEED_COMPATIBLE,
    0x7a5e4c01, 0x1d2b, 0x4f6a, 0x9a, 0x01, 0x54, 0x55, 0x52, 0x00, 0x00, 0x05);
DEFINE_GUID(GUID_USB_CAPABILITY_DEVICE_CONNECTION_SUPER_SPEED_COMPATIBLE,
    0x7a5e4c01, 0x1d2b, 0x4f6a, 0x9a, 0x01, 0x54, 0x55, 0x52, 0x00, 0x00, 0x06);
DEFINE_GUID(GUID_USB_CAPABILITY_SSP_ISOCH_PIPE_FLAGS,
    0x7a5e4c01, 0x1d2b, 0x4f6a, 0x9a, 0x01, 0x54, 0x55, 0x52, 0x00, 0x00, 0x07);

#endif /* _TUR_USBDLIB_ADDITIONS_ */
