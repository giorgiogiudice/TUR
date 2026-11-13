// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// hidtest.exe: a HID-using Windows application, for the HID path of the
// windows-drivers experiment: Linux usbhid/hidraw -> Wine's winebus.sys ->
// hidclass.sys -> hid.dll -> this program. No Windows driver of ours is
// involved; this is the path vendor tools for HID devices use.
//
//   hidtest.exe VID PID WAIT_SECONDS
//
// Finds the HID collection with this VID/PID, prints its attributes and
// capabilities, reads one input report (the gadget peer sends reports that
// start with "TURHID") and writes one output report that starts with
// "TUROUT" (the harness checks the gadget received it).
//
// Output: "HID info ..." and "HID check NAME PASS|FAIL DETAIL" lines; exit
// status 0 every check passed, 1 a check failed, 3 no device found.
#include <windows.h>
#include <setupapi.h>
#include <hidsdi.h>
#include <hidpi.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int failures;

static void check(const char *name, int ok, const char *fmt, ...)
{
    va_list args;
    printf("HID check %s %s ", name, ok ? "PASS" : "FAIL");
    va_start(args, fmt);
    vprintf(fmt, args);
    va_end(args);
    printf("\n");
    if (!ok)
        failures++;
}

static HANDLE find_hid(unsigned vid, unsigned pid, int wait_seconds)
{
    DWORD deadline = GetTickCount() + (DWORD)wait_seconds * 1000;
    GUID hid_guid;

    HidD_GetHidGuid(&hid_guid);
    for (;;) {
        HDEVINFO set = SetupDiGetClassDevsW(&hid_guid, NULL, NULL, DIGCF_DEVICEINTERFACE | DIGCF_PRESENT);
        SP_DEVICE_INTERFACE_DATA iface = {sizeof(iface)};
        DWORD i, seen = 0;

        if (set != INVALID_HANDLE_VALUE) {
            for (i = 0; SetupDiEnumDeviceInterfaces(set, NULL, &hid_guid, i, &iface); i++) {
                union {
                    SP_DEVICE_INTERFACE_DETAIL_DATA_W detail;
                    BYTE raw[sizeof(SP_DEVICE_INTERFACE_DETAIL_DATA_W) + 1024];
                } u;
                HIDD_ATTRIBUTES attributes = {sizeof(attributes)};
                HANDLE device;

                u.detail.cbSize = sizeof(u.detail);
                if (!SetupDiGetDeviceInterfaceDetailW(set, &iface, &u.detail, sizeof(u), NULL, NULL))
                    continue;
                seen++;
                device = CreateFileW(u.detail.DevicePath, GENERIC_READ | GENERIC_WRITE,
                                     FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_EXISTING, FILE_FLAG_OVERLAPPED,
                                     NULL);
                if (device == INVALID_HANDLE_VALUE) {
                    printf("HID info open-failed path=%ls error=%lu\n", u.detail.DevicePath, GetLastError());
                    continue;
                }
                if (HidD_GetAttributes(device, &attributes)) {
                    printf("HID info path=%ls vid=%04x pid=%04x version=%04x\n", u.detail.DevicePath,
                           attributes.VendorID, attributes.ProductID, attributes.VersionNumber);
                    if (attributes.VendorID == vid && attributes.ProductID == pid) {
                        SetupDiDestroyDeviceInfoList(set);
                        return device;
                    }
                }
                CloseHandle(device);
            }
            SetupDiDestroyDeviceInfoList(set);
        }
        if ((LONG)(GetTickCount() - deadline) > 0) {
            printf("HID info no-device vid=%04x pid=%04x hid-interfaces-seen=%lu\n", vid, pid, seen);
            return INVALID_HANDLE_VALUE;
        }
        Sleep(500);
    }
}

int main(int argc, char **argv)
{
    PHIDP_PREPARSED_DATA preparsed = NULL;
    OVERLAPPED overlapped = {0};
    BYTE report[256];
    WCHAR product[128] = L"";
    HIDP_CAPS caps;
    DWORD done = 0;
    unsigned vid, pid;
    HANDLE device;
    BOOL ok;
    int i, found_magic = 0;

    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc < 4) {
        printf("usage: hidtest.exe VID PID WAIT_SECONDS\n");
        return 2;
    }
    vid = strtoul(argv[1], NULL, 16);
    pid = strtoul(argv[2], NULL, 16);
    device = find_hid(vid, pid, atoi(argv[3]));
    if (device == INVALID_HANDLE_VALUE) {
        check("device-found", 0, "no HID device %04x:%04x", vid, pid);
        return 3;
    }
    check("device-found", 1, "%04x:%04x", vid, pid);

    if (HidD_GetProductString(device, product, sizeof(product)))
        printf("HID info product=%ls\n", product);
    ok = HidD_GetPreparsedData(device, &preparsed) && HidP_GetCaps(preparsed, &caps) == HIDP_STATUS_SUCCESS;
    check("caps", ok && caps.UsagePage == 0xff00,
          "usage-page=%04x usage=%04x input=%u output=%u feature=%u", ok ? caps.UsagePage : 0, ok ? caps.Usage : 0,
          ok ? caps.InputReportByteLength : 0, ok ? caps.OutputReportByteLength : 0,
          ok ? caps.FeatureReportByteLength : 0);
    if (!ok) {
        CloseHandle(device);
        return 1;
    }

    overlapped.hEvent = CreateEventW(NULL, TRUE, FALSE, NULL);
    // A few reports, in case the first is one queued before we opened.
    for (i = 0; i < 3 && !found_magic; i++) {
        memset(report, 0, sizeof(report));
        ResetEvent(overlapped.hEvent);
        ok = ReadFile(device, report, caps.InputReportByteLength, &done, &overlapped);
        if (!ok && GetLastError() == ERROR_IO_PENDING) {
            if (WaitForSingleObject(overlapped.hEvent, 5000) == WAIT_OBJECT_0)
                ok = GetOverlappedResult(device, &overlapped, &done, FALSE);
            else
                CancelIo(device);
        }
        // Byte 0 is the report ID (0: the descriptor declares none).
        found_magic = ok && done >= 7 && !memcmp(report + 1, "TURHID", 6);
        printf("HID info input-report ok=%d bytes=%lu first=%.6s\n", ok, done, done >= 7 ? (char *)report + 1 : "");
    }
    check("input-report", found_magic, "an input report starting with TURHID");

    memset(report, 0, sizeof(report));
    memcpy(report + 1, "TUROUT-from-wine", 16);
    ResetEvent(overlapped.hEvent);
    ok = WriteFile(device, report, caps.OutputReportByteLength, &done, &overlapped);
    if (!ok && GetLastError() == ERROR_IO_PENDING) {
        if (WaitForSingleObject(overlapped.hEvent, 5000) == WAIT_OBJECT_0)
            ok = GetOverlappedResult(device, &overlapped, &done, FALSE);
        else
            CancelIo(device);
    }
    check("output-report-write", ok && done == caps.OutputReportByteLength, "bytes=%lu error=%lu", done,
          ok ? 0 : GetLastError());

    HidD_FreePreparsedData(preparsed);
    CloseHandle(device);
    printf("HID done failures=%d\n", failures);
    return failures ? 1 : 0;
}
