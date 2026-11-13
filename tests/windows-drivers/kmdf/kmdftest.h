// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// Interface between kmdftest.sys (a minimal KMDF USB driver written for the
// experiment) and kmdftest.exe.
#pragma once

// {6c1f3e6a-8d1b-4b1e-9f42-54555244460a}
#define KMDFTEST_INTERFACE_GUID_INIT \
    { 0x6c1f3e6a, 0x8d1b, 0x4b1e, { 0x9f, 0x42, 0x54, 0x55, 0x52, 0x44, 0x46, 0x0a } }

#define KMDFTEST_DEVICE_TYPE 0x8f00
#define KMDFTEST_IOCTL(n) (((ULONG)KMDFTEST_DEVICE_TYPE << 16) | ((ULONG)(n) << 2)) // METHOD_BUFFERED, FILE_ANY_ACCESS

// Out: struct kmdftest_info.
#define KMDFTEST_IOCTL_INFO KMDFTEST_IOCTL(0x900)
// In: bytes; out: the same bytes read back (bulk OUT then bulk IN).
#define KMDFTEST_IOCTL_ECHO KMDFTEST_IOCTL(0x901)
// In: bytes; out: read back (vendor control OUT 0x5b, then IN 0x5c).
#define KMDFTEST_IOCTL_VENDOR KMDFTEST_IOCTL(0x902)
// Out: struct kmdftest_selftest: a WDFDPC, a WDFTIMER and a WDFWORKITEM,
// each queued once and waited for.
#define KMDFTEST_IOCTL_SELFTEST KMDFTEST_IOCTL(0x903)

#define KMDFTEST_EVENT_PREPARE_HARDWARE 0x01
#define KMDFTEST_EVENT_D0_ENTRY         0x02
#define KMDFTEST_EVENT_SELECT_CONFIG    0x04
#define KMDFTEST_EVENT_PIPES            0x08

#pragma pack(push, 8)
struct kmdftest_info {
    char tag[16];                    // "TUR-KMDFTEST"
    unsigned long events;            // KMDFTEST_EVENT_* that happened
    long prepare_status;             // NTSTATUS of the USB set-up in EvtDevicePrepareHardware
    unsigned short vendor, product;
    unsigned char num_interfaces, num_pipes, bulk_in, bulk_out;
    unsigned long traits;            // WDF_USB_DEVICE_INFORMATION.Traits
    unsigned long usbd_version;      // WDF_USB_DEVICE_INFORMATION.UsbdVersionInformation.USBDI_Version
    unsigned long d0_entries;
};

struct kmdftest_selftest {
    long dpc_status, timer_status, workitem_status;  // NTSTATUS of the wait for each callback
    unsigned long dpc_runs, timer_runs, workitem_runs;
    unsigned long dpc_ms, timer_ms, workitem_ms;     // time to the callback
};
#pragma pack(pop)
