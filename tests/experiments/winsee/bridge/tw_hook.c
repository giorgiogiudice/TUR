/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * tur-winhook.dll: reads a window's menu from inside the application.
 *
 * Under Wine a menu belongs to the process that made it: GetMenuItemCount
 * and GetMenuItemInfo from another process fail (win32u, grab_menu_ptr:
 * "other process menu"). The bridge therefore installs TwHookProc as a
 * WH_CALLWNDPROC hook on the window's thread for the length of one
 * message; the system loads this library into the application, the hook
 * sees the bridge's registered message, walks the window's menu there and
 * writes it to a file the bridge reads. It reads; it changes nothing in
 * the application and stays no longer than the hook.
 *
 * The file, one item a line, UTF-16:
 *   depth TAB path TAB command TAB type TAB state TAB submenu TAB
 *   left TAB top TAB right TAB bottom TAB text
 * `path` is the item's positions from the menu bar down, joined by dots.
 * The rectangle is in screen coordinates and given for the menu bar's own
 * items only.
 *
 * Experiment code (docs/reports/exp-winsee-1.md). Whether a library of
 * TUR's may be loaded into other publishers' programs is a licence
 * question the report puts to the owner.
 */
#ifndef UNICODE
#define UNICODE
#endif
#include <windows.h>

static UINT dump_message;

static void put(HANDLE file, const WCHAR *text)
{
    DWORD written;

    WriteFile(file, text, (DWORD)(lstrlenW(text) * sizeof(WCHAR)), &written, NULL);
}

static void put_number(HANDLE file, long value)
{
    WCHAR digits[24];
    int i = 23;
    unsigned long u = value < 0 ? (unsigned long)(-value) : (unsigned long)value;

    digits[i] = 0;
    do {
        digits[--i] = (WCHAR)(L'0' + u % 10);
        u /= 10;
    } while (u && i > 1);
    if (value < 0)
        digits[--i] = L'-';
    put(file, digits + i);
}

static void append(WCHAR *dst, int cap, const WCHAR *src)
{
    int have = lstrlenW(dst);

    if (have + 1 < cap)
        lstrcpynW(dst + have, src, cap - have);
}

static void dump(HANDLE file, HWND hwnd, HMENU menu, int depth, const WCHAR *path, int *budget)
{
    int count = GetMenuItemCount(menu), i;

    if (count < 0 || depth > 5)
        return;
    for (i = 0; i < count && *budget > 0; i++) {
        MENUITEMINFOW info;
        WCHAR text[300], sub[80], number[16];
        RECT rc;
        int n = i, d = 15;

        ZeroMemory(&info, sizeof info);
        info.cbSize = sizeof info;
        info.fMask = MIIM_FTYPE | MIIM_STATE | MIIM_ID | MIIM_SUBMENU | MIIM_STRING;
        text[0] = 0;
        info.dwTypeData = text;
        info.cch = 299;
        if (!GetMenuItemInfoW(menu, (UINT)i, TRUE, &info))
            continue;
        (*budget)--;
        number[d] = 0;
        do {
            number[--d] = (WCHAR)(L'0' + n % 10);
            n /= 10;
        } while (n && d > 0);
        sub[0] = 0;
        append(sub, 80, path);
        if (path[0])
            append(sub, 80, L".");
        append(sub, 80, number + d);

        SetRectEmpty(&rc);
        if (depth == 0)
            GetMenuItemRect(hwnd, menu, (UINT)i, &rc);
        put_number(file, depth);
        put(file, L"\t");
        put(file, sub);
        put(file, L"\t");
        put_number(file, (long)info.wID);
        put(file, L"\t");
        put_number(file, (long)info.fType);
        put(file, L"\t");
        put_number(file, (long)info.fState);
        put(file, L"\t");
        put(file, info.hSubMenu ? L"1" : L"0");
        put(file, L"\t");
        put_number(file, rc.left);
        put(file, L"\t");
        put_number(file, rc.top);
        put(file, L"\t");
        put_number(file, rc.right);
        put(file, L"\t");
        put_number(file, rc.bottom);
        put(file, L"\t");
        put(file, text);
        put(file, L"\n");
        if (info.hSubMenu)
            dump(file, hwnd, info.hSubMenu, depth + 1, sub, budget);
    }
}

static void dump_window(HWND hwnd, DWORD token)
{
    WCHAR path[MAX_PATH], digits[2 * sizeof(DWORD) + 1];
    DWORD n = GetTempPathW(MAX_PATH, path);
    HANDLE file;
    HMENU menu = GetMenu(hwnd);
    int i = (int)(sizeof digits / sizeof digits[0]) - 1, budget = 1500;

    if (!n || n >= MAX_PATH)
        lstrcpynW(path, L"C:\\", MAX_PATH);
    digits[i] = 0;
    do {
        digits[--i] = L"0123456789abcdef"[token & 15];
        token >>= 4;
    } while (token && i > 0);
    append(path, MAX_PATH, L"tur-winbridge-menu-");
    append(path, MAX_PATH, digits + i);
    append(path, MAX_PATH, L".txt");
    file = CreateFileW(path, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE)
        return;
    if (menu)
        dump(file, hwnd, menu, 0, L"", &budget);
    CloseHandle(file);
}

__declspec(dllexport) LRESULT CALLBACK TwHookProc(int code, WPARAM wparam, LPARAM lparam)
{
    if (code == HC_ACTION && lparam) {
        const CWPSTRUCT *call = (const CWPSTRUCT *)lparam;

        if (!dump_message)
            dump_message = RegisterWindowMessageW(L"TUR_WINBRIDGE_MENU_DUMP");
        if (dump_message && call->message == dump_message)
            dump_window(call->hwnd, (DWORD)call->wParam);
    }
    return CallNextHookEx(NULL, code, wparam, lparam);
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved)
{
    (void)instance;
    (void)reason;
    (void)reserved;
    return TRUE;
}
