/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * A deliberately broken program for the compatibility autopilot's CI
 * (tests/windows-runtime/autopilot.py): it refuses to run unless the
 * Windows version it is told is 11 or later (build 22000), as programs that
 * check the version do. Wine 9.0 reports Windows 10 (build 19043) by
 * default. It asks RtlGetVersion, which no compatibility manifest changes.
 */
#include <windows.h>
#include <stdio.h>

typedef LONG(WINAPI *rtl_get_version)(OSVERSIONINFOEXW *);

int main(void)
{
    OSVERSIONINFOEXW info;
    rtl_get_version get;

    ZeroMemory(&info, sizeof(info));
    info.dwOSVersionInfoSize = sizeof(info);
    get = (rtl_get_version)(void *)GetProcAddress(GetModuleHandleW(L"ntdll.dll"), "RtlGetVersion");
    if (get == NULL || get(&info) != 0) {
        fprintf(stderr, "cannot read the Windows version\n");
        return 2;
    }
    printf("Windows %lu.%lu build %lu\n", info.dwMajorVersion, info.dwMinorVersion,
           info.dwBuildNumber);
    fflush(stdout);
    if (info.dwMajorVersion < 10 || info.dwBuildNumber < 22000) {
        fprintf(stderr, "This program requires Windows 11 or later (found %lu.%lu build %lu).\n",
                info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber);
        return 3;
    }
    printf("TUR-CHECK-PASS winver\n");
    return 0;
}
