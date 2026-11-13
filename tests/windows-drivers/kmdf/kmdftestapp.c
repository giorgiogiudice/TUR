// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// kmdftest.exe: talks to kmdftest.sys, the minimal KMDF driver, through its
// device interface (SetupDiGetClassDevs + CreateFile + DeviceIoControl, as
// any Windows program does).
//
//   kmdftest.exe WAIT_SECONDS
//
// Output: "KMDF info ..." and "KMDF check NAME PASS|FAIL DETAIL" lines.
// Exit status: 0 every check passed, 1 a check failed, 3 no device found.
#include <windows.h>
#include <setupapi.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "kmdftest.h"

static const GUID kmdftest_guid = KMDFTEST_INTERFACE_GUID_INIT;
static int failures;

static void check(const char *name, int ok, const char *fmt, ...)
{
    va_list args;
    printf("KMDF check %s %s ", name, ok ? "PASS" : "FAIL");
    va_start(args, fmt);
    vprintf(fmt, args);
    va_end(args);
    printf("\n");
    fflush(stdout);
    if (!ok)
        failures++;
}

static HANDLE find_device(int wait_seconds)
{
    DWORD deadline = GetTickCount() + wait_seconds * 1000;

    for (;;) {
        HDEVINFO set = SetupDiGetClassDevsW(&kmdftest_guid, NULL, NULL, DIGCF_DEVICEINTERFACE | DIGCF_PRESENT);
        SP_DEVICE_INTERFACE_DATA iface = {sizeof(iface)};

        if (set != INVALID_HANDLE_VALUE) {
            if (SetupDiEnumDeviceInterfaces(set, NULL, &kmdftest_guid, 0, &iface)) {
                union {
                    SP_DEVICE_INTERFACE_DETAIL_DATA_W detail;
                    BYTE raw[sizeof(SP_DEVICE_INTERFACE_DETAIL_DATA_W) + 1024];
                } u;
                HANDLE device;

                u.detail.cbSize = sizeof(u.detail);
                if (SetupDiGetDeviceInterfaceDetailW(set, &iface, &u.detail, sizeof(u), NULL, NULL)) {
                    device = CreateFileW(u.detail.DevicePath, GENERIC_READ | GENERIC_WRITE,
                                         FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_EXISTING, 0, NULL);
                    printf("KMDF info path=%ls open=%s error=%lu\n", u.detail.DevicePath,
                           device == INVALID_HANDLE_VALUE ? "failed" : "ok", GetLastError());
                    if (device != INVALID_HANDLE_VALUE) {
                        SetupDiDestroyDeviceInfoList(set);
                        return device;
                    }
                }
            }
            SetupDiDestroyDeviceInfoList(set);
        }
        if ((LONG)(GetTickCount() - deadline) > 0)
            return INVALID_HANDLE_VALUE;
        Sleep(500);
    }
}

int main(int argc, char **argv)
{
    int wait_seconds = argc > 1 ? atoi(argv[1]) : 20;
    struct kmdftest_info info;
    struct kmdftest_selftest self;
    unsigned char out[512], in[512];
    DWORD returned = 0;
    HANDLE device;
    int i, ok;

    setvbuf(stdout, NULL, _IONBF, 0);
    device = find_device(wait_seconds);
    check("device-interface-found", device != INVALID_HANDLE_VALUE,
          "kmdftest.sys's device interface, within %d s", wait_seconds);
    if (device == INVALID_HANDLE_VALUE)
        return 3;

    memset(&info, 0, sizeof(info));
    ok = DeviceIoControl(device, KMDFTEST_IOCTL_INFO, NULL, 0, &info, sizeof(info), &returned, NULL);
    check("ioctl-info", ok && returned == sizeof(info), "returned %lu bytes, error %lu", returned,
          ok ? 0 : GetLastError());
    if (ok) {
        printf("KMDF info tag=%.16s events=%#lx prepare_status=%#lx vid=%04x pid=%04x interfaces=%u pipes=%u "
               "bulk_in=%02x bulk_out=%02x traits=%#lx usbdi=%#lx d0_entries=%lu\n",
               info.tag, info.events, (unsigned long)info.prepare_status, info.vendor, info.product,
               info.num_interfaces, info.num_pipes, info.bulk_in, info.bulk_out, info.traits, info.usbd_version,
               info.d0_entries);
        check("prepare-hardware-ran", info.events & KMDFTEST_EVENT_PREPARE_HARDWARE, "events %#lx", info.events);
        check("d0-entry-ran", info.events & KMDFTEST_EVENT_D0_ENTRY, "%lu D0 entries", info.d0_entries);
        check("usb-target-and-config", info.prepare_status == 0 && (info.events & KMDFTEST_EVENT_SELECT_CONFIG),
              "prepare status %#lx, device %04x:%04x", (unsigned long)info.prepare_status, info.vendor,
              info.product);
        check("pipes", info.events & KMDFTEST_EVENT_PIPES, "bulk in %02x, bulk out %02x", info.bulk_in,
              info.bulk_out);
    }

    memset(&self, 0, sizeof(self));
    returned = 0;
    ok = DeviceIoControl(device, KMDFTEST_IOCTL_SELFTEST, NULL, 0, &self, sizeof(self), &returned, NULL);
    check("ioctl-selftest", ok, "error %lu", ok ? 0 : GetLastError());
    if (ok) {
        check("wdfdpc-ran", self.dpc_status == 0 && self.dpc_runs == 1, "wait %#lx, runs %lu, %lu ms",
              (unsigned long)self.dpc_status, self.dpc_runs, self.dpc_ms);
        check("wdftimer-fired", self.timer_status == 0 && self.timer_runs >= 1, "wait %#lx, runs %lu, %lu ms",
              (unsigned long)self.timer_status, self.timer_runs, self.timer_ms);
        check("wdfworkitem-ran", self.workitem_status == 0 && self.workitem_runs == 1, "wait %#lx, runs %lu, %lu ms",
              (unsigned long)self.workitem_status, self.workitem_runs, self.workitem_ms);
    }

    for (i = 0; i < (int)sizeof(out); i++)
        out[i] = (unsigned char)(i * 7 + 3);
    memcpy(in, out, sizeof(out));
    returned = 0;
    ok = DeviceIoControl(device, KMDFTEST_IOCTL_ECHO, in, 64, in, 64, &returned, NULL);
    check("bulk-echo-64", ok && returned == 64 && !memcmp(in, out, 64), "returned %lu, error %lu", returned,
          ok ? 0 : GetLastError());
    memcpy(in, out, sizeof(out));
    returned = 0;
    ok = DeviceIoControl(device, KMDFTEST_IOCTL_ECHO, in, 512, in, 512, &returned, NULL);
    check("bulk-echo-512", ok && returned == 512 && !memcmp(in, out, 512), "returned %lu, error %lu", returned,
          ok ? 0 : GetLastError());
    memcpy(in, "TUR vendor request through KMDF", 32);
    returned = 0;
    ok = DeviceIoControl(device, KMDFTEST_IOCTL_VENDOR, in, 32, in, 32, &returned, NULL);
    check("vendor-control", ok && returned == 32 && !memcmp(in, "TUR vendor request through KMDF", 32),
          "returned %lu, error %lu", returned, ok ? 0 : GetLastError());

    CloseHandle(device);
    return failures ? 1 : 0;
}
