/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * winwindow.exe: a small Windows program of TUR's own for the Windows path
 * of P@x's accessibility test (tests/pax/a11y/harness.py, phase "windows";
 * docs/architecture/pax-accessibility.md, section 11). Built with mingw-w64
 * in the test's container; run under Wine with tur-runtime's supervisor
 * offering P@x's bridge beside it.
 *
 *   winwindow.exe LOGFILE
 *
 * The window "TUR Windows test" has a menu (File: New, Exit), the push
 * buttons Preview, Delete and OK (the default button), and the fields Name
 * and Password with their labels. Everything done to it is written to
 * LOGFILE, one line each ("ready", "clicked:Preview", "menu:New",
 * "text:Name:hello"), so that the test can tell what arrived in the
 * application, not only what the bridge said.
 */
#ifndef UNICODE
#define UNICODE
#endif
#include <windows.h>

enum {
    ID_NAME = 101,
    ID_PASSWORD,
    ID_PREVIEW,
    ID_DELETE,
    ID_OK,
    IDM_NEW = 201,
    IDM_EXIT
};

static HANDLE log_file = INVALID_HANDLE_VALUE;

static void log_line(const WCHAR *text)
{
    char utf8[1200];
    DWORD written;
    int n = WideCharToMultiByte(CP_UTF8, 0, text, -1, utf8, (int)sizeof utf8 - 2, NULL, NULL);

    if (log_file == INVALID_HANDLE_VALUE || n <= 0)
        return;
    utf8[n - 1] = '\n';
    WriteFile(log_file, utf8, (DWORD)n, &written, NULL);
    FlushFileBuffers(log_file);
}

static void log_pair(const WCHAR *what, const WCHAR *value)
{
    WCHAR line[600];

    lstrcpynW(line, what, 100);
    lstrcatW(line, L":");
    lstrcpynW(line + lstrlenW(line), value, 480);
    log_line(line);
}

static HWND make(HWND parent, const WCHAR *cls, const WCHAR *text, DWORD style, int x, int y, int w, int h, int id)
{
    HWND control = CreateWindowExW(0, cls, text, WS_CHILD | WS_VISIBLE | style, x, y, w, h, parent,
                                   (HMENU)(INT_PTR)id, GetModuleHandleW(NULL), NULL);

    SendMessageW(control, WM_SETFONT, (WPARAM)GetStockObject(DEFAULT_GUI_FONT), TRUE);
    return control;
}

static LRESULT CALLBACK window_proc(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam)
{
    switch (message) {
    case WM_CREATE:
        make(hwnd, L"STATIC", L"Name:", 0, 16, 18, 80, 20, 0);
        make(hwnd, L"EDIT", L"", WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL, 100, 16, 220, 22, ID_NAME);
        make(hwnd, L"STATIC", L"Password:", 0, 16, 52, 80, 20, 0);
        make(hwnd, L"EDIT", L"", WS_BORDER | WS_TABSTOP | ES_PASSWORD | ES_AUTOHSCROLL, 100, 50, 220, 22, ID_PASSWORD);
        make(hwnd, L"BUTTON", L"&Preview", WS_TABSTOP | BS_PUSHBUTTON, 16, 96, 96, 28, ID_PREVIEW);
        make(hwnd, L"BUTTON", L"Delete", WS_TABSTOP | BS_PUSHBUTTON, 124, 96, 96, 28, ID_DELETE);
        make(hwnd, L"BUTTON", L"OK", WS_TABSTOP | BS_DEFPUSHBUTTON, 232, 96, 96, 28, ID_OK);
        log_line(L"ready");
        return 0;
    case WM_COMMAND: {
        int id = LOWORD(wparam), code = HIWORD(wparam);
        WCHAR text[480];

        if (id == ID_NAME && code == EN_CHANGE) {
            GetWindowTextW((HWND)lparam, text, 480);
            log_pair(L"text:Name", text);
        } else if (id == ID_PASSWORD && code == EN_CHANGE) {
            log_line(L"text:Password:(changed)");
        } else if (id == ID_PREVIEW && code == BN_CLICKED) {
            log_line(L"clicked:Preview");
        } else if (id == ID_DELETE && code == BN_CLICKED) {
            log_line(L"clicked:Delete");
        } else if (id == ID_OK && code == BN_CLICKED) {
            log_line(L"clicked:OK");
        } else if (id == IDM_NEW) {
            log_line(L"menu:New");
        } else if (id == IDM_EXIT) {
            log_line(L"menu:Exit");
            DestroyWindow(hwnd);
        }
        return 0;
    }
    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;
    default:
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
}

int WINAPI wWinMain(HINSTANCE instance, HINSTANCE previous, PWSTR command_line, int show)
{
    WNDCLASSW cls;
    HMENU bar = CreateMenu(), file = CreatePopupMenu();
    HWND hwnd;
    MSG message;
    int argc = 0;
    WCHAR **argv = CommandLineToArgvW(GetCommandLineW(), &argc);

    if (argv && argc > 1)
        log_file = CreateFileW(argv[1], FILE_APPEND_DATA, FILE_SHARE_READ, NULL, OPEN_ALWAYS, 0, NULL);
    AppendMenuW(file, MF_STRING, IDM_NEW, L"&New\tCtrl+N");
    AppendMenuW(file, MF_STRING, IDM_EXIT, L"E&xit");
    AppendMenuW(bar, MF_POPUP, (UINT_PTR)file, L"&File");

    ZeroMemory(&cls, sizeof cls);
    cls.lpfnWndProc = window_proc;
    cls.hInstance = instance;
    cls.hCursor = LoadCursorW(NULL, (LPCWSTR)IDC_ARROW);
    cls.hbrBackground = (HBRUSH)(COLOR_BTNFACE + 1);
    cls.lpszClassName = L"TurWindowsTest";
    RegisterClassW(&cls);
    hwnd = CreateWindowExW(0, L"TurWindowsTest", L"TUR Windows test", WS_OVERLAPPEDWINDOW, 80, 80, 360, 200, NULL,
                           bar, instance, NULL);
    ShowWindow(hwnd, SW_SHOWNORMAL);
    UpdateWindow(hwnd);
    while (GetMessageW(&message, NULL, 0, 0) > 0) {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    return 0;
}
