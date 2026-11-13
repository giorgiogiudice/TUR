/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * A deliberately broken program for the compatibility autopilot's CI: like
 * old games that assume a display mode, it refuses a screen larger than
 * 1024x768. The CI's X server is 1280x800. In a Wine virtual desktop of
 * 1024x768 the screen is that size (programs/explorer/desktop.c).
 */
#include <windows.h>
#include <stdio.h>

int main(void)
{
    int width = GetSystemMetrics(SM_CXSCREEN);
    int height = GetSystemMetrics(SM_CYSCREEN);
    HWND window;
    MSG message;
    DWORD until;

    printf("screen %dx%d\n", width, height);
    fflush(stdout);
    if (width > 1024 || height > 768) {
        fprintf(stderr,
                "The display resolution %dx%d is not supported: this program needs a screen of "
                "at most 1024x768.\n",
                width, height);
        return 6;
    }
    window = CreateWindowExA(0, "STATIC", "TUR autopilot vdesktop", WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                             10, 10, 320, 200, NULL, NULL, GetModuleHandleA(NULL), NULL);
    if (window == NULL) {
        fprintf(stderr, "cannot create a window\n");
        return 7;
    }
    until = GetTickCount() + 1000;
    while (GetTickCount() < until) {
        while (PeekMessageA(&message, NULL, 0, 0, PM_REMOVE)) {
            TranslateMessage(&message);
            DispatchMessageA(&message);
        }
        Sleep(20);
    }
    DestroyWindow(window);
    printf("TUR-CHECK-PASS vdesktop\n");
    return 0;
}
