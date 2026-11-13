// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// winusbtest.exe: what a WinUSB-based vendor tool does first, against Wine's
// own winusb.dll (no Microsoft file is involved).
//
// Most current vendor USB tools need no vendor driver at all on Windows:
// Microsoft's WinUSB (winusb.sys bound to the device, winusb.dll in the
// program) moves the data. On Windows the device path would be the WinUSB
// device interface; here it is the device interface of twdusb.sys on the
// loopback gadget, which is enough to reach WinUsb_Initialize.
//
//   winusbtest.exe WAIT_SECONDS
//
// Output: "WINUSB ..." lines and "WINUSB check NAME PASS|FAIL DETAIL".
#include <windows.h>
#include <winioctl.h>
#include <setupapi.h>
#include <winusb.h>
#include <stdio.h>
#include <stdlib.h>

#include "../driver/twdusb.h"

static const GUID twd_guid = TWD_INTERFACE_GUID_INIT;

int main(int argc, char **argv)
{
    int wait_seconds = argc > 1 ? atoi(argv[1]) : 30;
    DWORD deadline = GetTickCount() + (DWORD)wait_seconds * 1000;
    WINUSB_INTERFACE_HANDLE usb = NULL;
    HANDLE device = INVALID_HANDLE_VALUE;
    BOOL ok;

    setvbuf(stdout, NULL, _IONBF, 0);
    while (device == INVALID_HANDLE_VALUE) {
        HDEVINFO set = SetupDiGetClassDevsW(&twd_guid, NULL, NULL, DIGCF_DEVICEINTERFACE | DIGCF_PRESENT);
        SP_DEVICE_INTERFACE_DATA iface = {sizeof(iface)};
        if (set != INVALID_HANDLE_VALUE && SetupDiEnumDeviceInterfaces(set, NULL, &twd_guid, 0, &iface)) {
            union {
                SP_DEVICE_INTERFACE_DETAIL_DATA_W detail;
                BYTE raw[sizeof(SP_DEVICE_INTERFACE_DETAIL_DATA_W) + 1024];
            } u;
            u.detail.cbSize = sizeof(u.detail);
            if (SetupDiGetDeviceInterfaceDetailW(set, &iface, &u.detail, sizeof(u), NULL, NULL))
                device = CreateFileW(u.detail.DevicePath, GENERIC_READ | GENERIC_WRITE,
                                     FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_EXISTING, FILE_FLAG_OVERLAPPED,
                                     NULL);
        }
        if (set != INVALID_HANDLE_VALUE)
            SetupDiDestroyDeviceInfoList(set);
        if (device == INVALID_HANDLE_VALUE) {
            if ((LONG)(GetTickCount() - deadline) > 0) {
                printf("WINUSB check device-found FAIL no device handle\n");
                return 3;
            }
            Sleep(500);
        }
    }
    printf("WINUSB check device-found PASS a device handle to pass to WinUsb_Initialize\n");
    printf("WINUSB calling WinUsb_Initialize\n");
    ok = WinUsb_Initialize(device, &usb);
    printf("WINUSB check WinUsb_Initialize %s ok=%d error=%lu\n", ok ? "PASS" : "FAIL", ok, ok ? 0 : GetLastError());
    if (ok)
        WinUsb_Free(usb);
    CloseHandle(device);
    return ok ? 0 : 1;
}
