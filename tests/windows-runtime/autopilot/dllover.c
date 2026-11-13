/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * A deliberately broken program for the compatibility autopilot's CI: it
 * imports D3DXFloat32To16Array from d3dx9_43.dll, and the copy it ships
 * beside itself (broken-d3dx9.c) fails to initialise. Wine's own d3dx9_43
 * implements the function. Built with -DTUR_ALSO_VERSION it is combo.exe,
 * which also refuses any Windows older than 11: two remedies are needed.
 */
#include <windows.h>
#include <stdio.h>

__declspec(dllimport) void *WINAPI D3DXFloat32To16Array(WORD *out, const float *in, UINT count);

#ifdef TUR_ALSO_VERSION
typedef LONG(WINAPI *rtl_get_version)(OSVERSIONINFOEXW *);

static int windows_11_or_later(void)
{
    OSVERSIONINFOEXW info;
    rtl_get_version get;

    ZeroMemory(&info, sizeof(info));
    info.dwOSVersionInfoSize = sizeof(info);
    get = (rtl_get_version)(void *)GetProcAddress(GetModuleHandleW(L"ntdll.dll"), "RtlGetVersion");
    if (get == NULL || get(&info) != 0)
        return 0;
    if (info.dwMajorVersion < 10 || info.dwBuildNumber < 22000) {
        fprintf(stderr, "This program requires Windows 11 or later (found %lu.%lu build %lu).\n",
                info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber);
        return 0;
    }
    return 1;
}
#endif

int main(void)
{
    float in[2] = {1.0f, -2.0f};
    WORD out[2] = {0, 0};

    D3DXFloat32To16Array(out, in, 2);
    printf("half floats: %04x %04x\n", out[0], out[1]);
    fflush(stdout);
    if (out[0] != 0x3c00 || out[1] != 0xc000) {
        fprintf(stderr, "the half-precision conversion is wrong\n");
        return 4;
    }
#ifdef TUR_ALSO_VERSION
    if (!windows_11_or_later())
        return 3;
    printf("TUR-CHECK-PASS combo\n");
#else
    printf("TUR-CHECK-PASS dllover\n");
#endif
    return 0;
}
