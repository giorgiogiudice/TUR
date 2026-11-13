// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// twdinst.exe: installs a driver package the ways a Windows installer does,
// and lists what Wine's PnP manager knows, for the windows-drivers harness.
//
//   twdinst.exe copy INF            SetupCopyOEMInfW: stage INF as oemN.inf
//   twdinst.exe update INF HWID     UpdateDriverForPlugAndPlayDevicesW
//   twdinst.exe diinstall INF       DiInstallDriverW (newdev)
//   twdinst.exe files               the staged INFs and installed .sys files
//   twdinst.exe devices             every device setupapi knows whose
//                                   instance ID starts with "USB\" or "HID\"
//
// Every line it prints starts with "TWDINST ". Exit status 0 when the call
// it was asked to make reported success.
#include <windows.h>
#include <setupapi.h>
#include <newdev.h>
#include <cfgmgr32.h>
#include <stdio.h>
#include <string.h>
#include <wchar.h>

static void list_files(const wchar_t *pattern)
{
    WIN32_FIND_DATAW data;
    HANDLE find = FindFirstFileW(pattern, &data);
    if (find == INVALID_HANDLE_VALUE) {
        wprintf(L"TWDINST file-none %ls\n", pattern);
        return;
    }
    do {
        wprintf(L"TWDINST file %ls %lu\n", data.cFileName, data.nFileSizeLow);
    } while (FindNextFileW(find, &data));
    FindClose(find);
}

static void print_property(HDEVINFO set, SP_DEVINFO_DATA *dev, DWORD property, const wchar_t *name)
{
    wchar_t buffer[2048];
    DWORD type = 0, size = 0;
    if (!SetupDiGetDeviceRegistryPropertyW(set, dev, property, &type, (BYTE *)buffer, sizeof(buffer) - 4, &size)) {
        wprintf(L"TWDINST   %ls: (none, error %lu)\n", name, GetLastError());
        return;
    }
    buffer[size / sizeof(wchar_t)] = 0;
    buffer[size / sizeof(wchar_t) + 1] = 0;
    if (type == REG_MULTI_SZ) {
        wchar_t *p;
        for (p = buffer; *p; p += wcslen(p) + 1)
            wprintf(L"TWDINST   %ls: %ls\n", name, p);
    } else if (type == REG_SZ) {
        wprintf(L"TWDINST   %ls: %ls\n", name, buffer);
    } else if (type == REG_DWORD) {
        DWORD value;
        memcpy(&value, buffer, sizeof(value));
        wprintf(L"TWDINST   %ls: %#lx\n", name, value);
    }
}

static int list_devices(void)
{
    HDEVINFO set = SetupDiGetClassDevsW(NULL, NULL, NULL, DIGCF_ALLCLASSES);
    SP_DEVINFO_DATA dev = {sizeof(dev)};
    wchar_t id[MAX_DEVICE_ID_LEN];
    DWORD i, shown = 0;

    if (set == INVALID_HANDLE_VALUE) {
        wprintf(L"TWDINST devices-error %lu\n", GetLastError());
        return 1;
    }
    for (i = 0; SetupDiEnumDeviceInfo(set, i, &dev); i++) {
        if (!SetupDiGetDeviceInstanceIdW(set, &dev, id, MAX_DEVICE_ID_LEN, NULL))
            continue;
        if (wcsncmp(id, L"USB\\", 4) && wcsncmp(id, L"HID\\", 4))
            continue;
        shown++;
        wprintf(L"TWDINST device %ls\n", id);
        print_property(set, &dev, SPDRP_HARDWAREID, L"hardware-id");
        print_property(set, &dev, SPDRP_SERVICE, L"service");
        print_property(set, &dev, SPDRP_DRIVER, L"driver-key");
        print_property(set, &dev, SPDRP_CLASS, L"class");
        print_property(set, &dev, SPDRP_DEVICEDESC, L"description");
    }
    wprintf(L"TWDINST devices-count %lu\n", shown);
    SetupDiDestroyDeviceInfoList(set);
    return 0;
}

int wmain(int argc, wchar_t **argv)
{
    wchar_t dest[MAX_PATH] = L"", full[MAX_PATH];
    BOOL reboot = FALSE, ok;

    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc < 2) {
        wprintf(L"TWDINST usage: copy INF | update INF HWID | diinstall INF | files | devices\n");
        return 2;
    }
    if (!wcscmp(argv[1], L"files")) {
        list_files(L"C:\\windows\\inf\\oem*.inf");
        list_files(L"C:\\windows\\system32\\drivers\\twdusb.sys");
        list_files(L"C:\\windows\\system32\\drivers\\kmdfprobe.sys");
        list_files(L"C:\\windows\\system32\\driverstore\\filerepository\\*");
        return 0;
    }
    if (!wcscmp(argv[1], L"devices"))
        return list_devices();
    if (argc < 3)
        return 2;
    if (!GetFullPathNameW(argv[2], MAX_PATH, full, NULL))
        wcscpy(full, argv[2]);

    if (!wcscmp(argv[1], L"copy")) {
        ok = SetupCopyOEMInfW(full, NULL, SPOST_PATH, 0, dest, MAX_PATH, NULL, NULL);
        wprintf(L"TWDINST SetupCopyOEMInfW %ls -> %ls ok=%d error=%lu\n", full, dest, ok, ok ? 0 : GetLastError());
        return ok ? 0 : 1;
    }
    if (!wcscmp(argv[1], L"update") && argc >= 4) {
        ok = UpdateDriverForPlugAndPlayDevicesW(NULL, argv[3], full, 0, &reboot);
        wprintf(L"TWDINST UpdateDriverForPlugAndPlayDevicesW %ls %ls ok=%d error=%lu reboot=%d\n", full, argv[3], ok,
                ok ? 0 : GetLastError(), reboot);
        return ok ? 0 : 1;
    }
    if (!wcscmp(argv[1], L"diinstall")) {
        ok = DiInstallDriverW(NULL, full, 0, &reboot);
        wprintf(L"TWDINST DiInstallDriverW %ls ok=%d error=%lu reboot=%d\n", full, ok, ok ? 0 : GetLastError(),
                reboot);
        list_files(L"C:\\windows\\inf\\oem*.inf");
        return ok ? 0 : 1;
    }
    wprintf(L"TWDINST unknown command %ls\n", argv[1]);
    return 2;
}
