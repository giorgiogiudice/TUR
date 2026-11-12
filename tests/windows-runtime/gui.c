// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// A minimal Windows GUI-subsystem program for the windows-runtime end-to-end
// test. It opens one window titled "TUR test window", keeps it for four
// seconds so that the test driver can find it on the X display, then closes
// it and exits with status 7.
//
// When TUR_TEST_GRANT_DIR is set (a Unix path) it writes gui-ran.txt there
// once the window exists.

#include <stdio.h>
#include <stdlib.h>
#include <windows.h>

static LRESULT CALLBACK window_proc(HWND window, UINT message, WPARAM wparam, LPARAM lparam)
{
    switch (message) {
    case WM_TIMER:
        DestroyWindow(window);
        return 0;
    case WM_DESTROY:
        PostQuitMessage(7);
        return 0;
    default:
        return DefWindowProcA(window, message, wparam, lparam);
    }
}

static void report(HWND window)
{
    const char *grant = getenv("TUR_TEST_GRANT_DIR");
    char path[2048];
    if (!grant)
        return;
    snprintf(path, sizeof path, "Z:%s/gui-ran.txt", grant);
    for (char *p = path; *p; ++p)
        if (*p == '/')
            *p = '\\';
    FILE *file = fopen(path, "wb");
    if (!file)
        return;
    fprintf(file, "window-created visible=%d screen=%dx%d\n", IsWindowVisible(window) ? 1 : 0,
            GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
    fclose(file);
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR command_line, int show)
{
    (void)previous;
    (void)command_line;
    (void)show;

    WNDCLASSA window_class = {0};
    window_class.lpfnWndProc = window_proc;
    window_class.hInstance = instance;
    window_class.lpszClassName = "TurTestWindow";
    window_class.hCursor = LoadCursorA(NULL, (LPCSTR)IDC_ARROW);
    if (!RegisterClassA(&window_class))
        return 2;

    HWND window = CreateWindowExA(0, "TurTestWindow", "TUR test window", WS_OVERLAPPEDWINDOW,
                                  CW_USEDEFAULT, CW_USEDEFAULT, 320, 200, NULL, NULL, instance,
                                  NULL);
    if (!window)
        return 3;
    ShowWindow(window, SW_SHOW);
    UpdateWindow(window);
    report(window);
    SetTimer(window, 1, 4000, NULL);

    MSG message;
    while (GetMessageA(&message, NULL, 0, 0) > 0) {
        TranslateMessage(&message);
        DispatchMessageA(&message);
    }
    return (int)message.wParam;
}
