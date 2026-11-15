// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// A stand-in for a runtime library an application needs and does not ship:
// turrt.dll, which the layers experiment puts into a runtime layer of its
// own (C:\windows\system32, with a registry key beside it). TUR's own code;
// built in CI with mingw-w64 and never committed.
#include <windows.h>

__declspec(dllexport) int tur_runtime_answer(void)
{
    return 42;
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved)
{
    (void)instance;
    (void)reason;
    (void)reserved;
    return TRUE;
}
