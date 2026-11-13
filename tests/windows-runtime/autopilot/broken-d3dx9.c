/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * Built as d3dx9_43.dll and shipped beside dllover.exe and combo.exe: a
 * program's own copy of a DLL that Wine also implements, and that does not
 * work. Its initialisation fails, as a copy built for another system's
 * driver or checking for something Wine does not have would. It is TUR's
 * own code; it only has the name of the DLL it stands in for, and exports
 * the one function the programs use.
 *
 * Wine marks its own d3dx9_43 "prefer native", so by default the copy
 * beside the program is loaded (dlls/d3dx9_43/Makefile.in, Wine 9.0).
 */
#include <windows.h>

__declspec(dllexport) void *WINAPI D3DXFloat32To16Array(WORD *out, const float *in, UINT count)
{
    UINT i;
    (void)in;
    for (i = 0; i < count; i++)
        out[i] = 0;
    return out;
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved)
{
    (void)instance;
    (void)reserved;
    /* The program's own copy refuses to load. */
    return reason == DLL_PROCESS_ATTACH ? FALSE : TRUE;
}
