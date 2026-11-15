// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// A program that needs turrt.dll (see turrt.c) and the registry value its
// runtime layer sets. It prints "answer 42 version 1.0" and exits with 0
// when both are there; without the DLL Wine cannot load it at all.
#include <stdio.h>
#include <windows.h>

__declspec(dllimport) int tur_runtime_answer(void);

int main(void)
{
    char version[64] = "(no registry value)";
    DWORD size = sizeof(version);
    LSTATUS status = RegGetValueA(HKEY_LOCAL_MACHINE, "Software\\TUR\\Runtime", "Version",
                                  RRF_RT_REG_SZ, NULL, version, &size);
    printf("answer %d version %s\n", tur_runtime_answer(), version);
    fflush(stdout);
    return status == ERROR_SUCCESS ? 0 : 3;
}
