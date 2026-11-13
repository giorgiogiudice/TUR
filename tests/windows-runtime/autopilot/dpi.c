/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * A deliberately broken program for the compatibility autopilot's CI: its
 * layout needs at least 120 logical pixels per inch (125% scaling) and it
 * refuses to start below that. Wine reports 96 unless LogPixels says
 * otherwise (dlls/win32u/sysparams.c). The program declares itself DPI-aware
 * so that it sees the system's value and not a virtualised 96.
 */
#include <windows.h>
#include <stdio.h>

int main(void)
{
    HDC screen;
    int dpi;

    SetProcessDPIAware();
    screen = GetDC(NULL);
    dpi = GetDeviceCaps(screen, LOGPIXELSX);
    ReleaseDC(NULL, screen);
    printf("logical pixels per inch: %d\n", dpi);
    fflush(stdout);
    if (dpi < 120) {
        fprintf(stderr,
                "Text would be clipped at %d DPI: this program needs display scaling of at least "
                "125%% (120 DPI).\n",
                dpi);
        return 7;
    }
    printf("TUR-CHECK-PASS dpi\n");
    return 0;
}
