/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * A deliberately broken program for the compatibility autopilot's CI: it
 * imports a function from mfc140u.dll, the Microsoft Foundation Classes of
 * the Visual C++ redistributable, which Wine does not implement and TUR
 * never supplies. The import library is made from mfc140u.def with
 * dlltool; no Microsoft file is used, and the function is never called.
 * The autopilot must say that TUR cannot supply the component and suggest
 * the Windows guest, and must try no remedy.
 */
#include <stdio.h>

__declspec(dllimport) int TurAutopilotNeverCalled(void);

int main(int argc, char **argv)
{
    (void)argv;
    if (argc > 1000)
        return TurAutopilotNeverCalled();
    printf("TUR-CHECK-PASS mfc\n");
    return 0;
}
