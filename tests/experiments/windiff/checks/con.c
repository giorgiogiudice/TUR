/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * windiff, console: modes, code pages, the screen buffer, input, control
 * events and console handles.
 *
 * Each group makes a pseudoconsole of 80 by 25 cells (CreatePseudoConsole)
 * and runs itself in it as a child, so that the console under test has the
 * same size and a known beginning on every machine and needs no window.
 * The child opens CONIN$ and CONOUT$ and writes its check lines to a file
 * the parent then copies to its output.
 */
#include "wd.h"
#include <wctype.h>

static HRESULT (WINAPI *pCreatePseudoConsole)(COORD, HANDLE, HANDLE, DWORD, void **);
static void (WINAPI *pClosePseudoConsole)(void *);
static HANDLE cin, cout;

static DWORD WINAPI drain(void *arg)
{
    static char buf[4096];
    DWORD n;
    while (ReadFile((HANDLE)arg, buf, sizeof(buf), &n, NULL) && n) ;
    return 0;
}

static void run_pty(void)
{
    HANDLE in_r, in_w, out_r, out_w, thread, h;
    void *pc = NULL;
    COORD size = {80, 25};
    HRESULT hr;
    STARTUPINFOEXW si;
    PROCESS_INFORMATION pi;
    SIZE_T sz = 0;
    static WCHAR cmd[1500], out[600], group[80];
    static char buf[16384];
    DWORD code = 0xffffffff, n;
    HMODULE k32 = GetModuleHandleW(L"kernel32.dll");

    pCreatePseudoConsole = (void *)GetProcAddress(k32, "CreatePseudoConsole");
    pClosePseudoConsole = (void *)GetProcAddress(k32, "ClosePseudoConsole");
    if (!pCreatePseudoConsole || !pClosePseudoConsole) { wd_emit("pseudoconsole.create", "missing-export"); return; }
    CreatePipe(&in_r, &in_w, NULL, 0);
    CreatePipe(&out_r, &out_w, NULL, 0);
    hr = pCreatePseudoConsole(size, in_r, out_w, 0, &pc);
    wd_emit("pseudoconsole.create", "hr=%08lx", (unsigned long)hr);
    if (FAILED(hr)) return;
    thread = CreateThread(NULL, 0, drain, out_r, 0, NULL);
    memset(&si, 0, sizeof(si));
    si.StartupInfo.cb = sizeof(si);
    InitializeProcThreadAttributeList(NULL, 1, 0, &sz);
    si.lpAttributeList = HeapAlloc(GetProcessHeap(), 0, sz);
    InitializeProcThreadAttributeList(si.lpAttributeList, 1, 0, &sz);
    UpdateProcThreadAttribute(si.lpAttributeList, 0, 0x00020016 /* PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE */, pc, sizeof(pc), NULL, NULL);
    GetTempPathW(500, out);
    lstrcatW(out, L"wdcon.out");
    DeleteFileW(out);
    MultiByteToWideChar(CP_UTF8, 0, wd_group, -1, group, 79);
    _snwprintf(cmd, 1490, L"\"%ls\" --child pty %ls \"%ls\"", wd_self, group, out);
    if (!CreateProcessW(NULL, cmd, NULL, NULL, FALSE, EXTENDED_STARTUPINFO_PRESENT, NULL, NULL, &si.StartupInfo, &pi))
        wd_emit("pseudoconsole.child", "e=%lu", GetLastError());
    else
    {
        if (WaitForSingleObject(pi.hProcess, 60000) != WAIT_OBJECT_0)
        {
            TerminateProcess(pi.hProcess, 0xfffffffe);
            WaitForSingleObject(pi.hProcess, 5000);
        }
        GetExitCodeProcess(pi.hProcess, &code);
        wd_emit("pseudoconsole.child", "code=0x%lx", code);
        CloseHandle(pi.hThread); CloseHandle(pi.hProcess);
    }
    CloseHandle(in_w);
    pClosePseudoConsole(pc);
    CloseHandle(in_r); CloseHandle(out_w);
    WaitForSingleObject(thread, 5000);
    CloseHandle(out_r);
    h = wd_open(out, GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    if (h != INVALID_HANDLE_VALUE)
    {
        while (ReadFile(h, buf, sizeof(buf), &n, NULL) && n) wd_write(buf, (int)n);
        CloseHandle(h);
        DeleteFileW(out);
    }
}

/* ---- in the pseudoconsole --------------------------------------------- */

static const char *cur(void)
{
    static char b[4][40];
    static int i;
    char *p = b[i++ & 3];
    CONSOLE_SCREEN_BUFFER_INFO info;
    if (!GetConsoleScreenBufferInfo(cout, &info)) _snprintf(p, 40, "e=%lu", GetLastError());
    else _snprintf(p, 40, "%d,%d", info.dwCursorPosition.X, info.dwCursorPosition.Y);
    return p;
}

static void wr(const WCHAR *s)
{
    DWORD n;
    WriteConsoleW(cout, s, lstrlenW(s), &n, NULL);
}

static void home(void)
{
    COORD c = {0, 0};
    DWORD n;
    FillConsoleOutputCharacterW(cout, ' ', 80 * 25, c, &n);
    FillConsoleOutputAttribute(cout, 7, 80 * 25, c, &n);
    SetConsoleCursorPosition(cout, c);
}

/* The first `len` cells of row y, trailing spaces removed. */
static const char *row(int y, int len)
{
    static WCHAR buf[200];
    DWORD n = 0;
    COORD c;
    c.X = 0; c.Y = (SHORT)y;
    if (!ReadConsoleOutputCharacterW(cout, buf, len, c, &n))
    {
        static char e[32];
        _snprintf(e, 32, "e=%lu", GetLastError());
        return e;
    }
    while (n && buf[n - 1] == ' ') n--;
    return wd_u8n(buf, (int)n);
}

static void c_modes(void)
{
    static const struct { const char *n; DWORD v; } in_modes[] = {
        {"0", 0}, {"processed", 1}, {"line", 2}, {"echo", 4}, {"echo-line", 6}, {"window", 8}, {"mouse", 0x10},
        {"insert", 0x20}, {"quick-edit", 0x40}, {"extended", 0x80}, {"quick-edit-extended", 0xc0},
        {"auto-position", 0x100}, {"vt-input", 0x200}, {"0x400", 0x400}, {"0x8000", 0x8000}, {"0xffff", 0xffff},
        {"0x1f7", 0x1f7},
    };
    static const struct { const char *n; DWORD v; } out_modes[] = {
        {"0", 0}, {"processed", 1}, {"wrap", 2}, {"processed-wrap", 3}, {"vt", 4}, {"vt-processed", 5},
        {"no-auto-return", 8}, {"lvb-grid", 0x10}, {"0x1f", 0x1f}, {"0x20", 0x20}, {"0xffff", 0xffff},
    };
    static const UINT cps[] = {437, 850, 1252, 65001, 932, 936, 1200, 1201, 65000, 0, 1, 12345, 20127, 28591, 37};
    unsigned i;
    DWORD m, saved_in = 0, saved_out = 0;
    BOOL ok;
    char id[80];
    HANDLE f;

    ok = GetConsoleMode(cin, &saved_in);
    wd_emit("input.default", ok ? "0x%lx" : "e=%lu", ok ? saved_in : GetLastError());
    ok = GetConsoleMode(cout, &saved_out);
    wd_emit("output.default", ok ? "0x%lx" : "e=%lu", ok ? saved_out : GetLastError());
    for (i = 0; i < ARRAY_SIZE(in_modes); i++)
    {
        SetConsoleMode(cin, saved_in);
        _snprintf(id, sizeof(id), "input.set-%s", in_modes[i].n);
        ok = SetConsoleMode(cin, in_modes[i].v);
        m = 0xdead;
        GetConsoleMode(cin, &m);
        wd_emit(id, "%s get=0x%lx", wd_res(ok), m);
    }
    SetConsoleMode(cin, saved_in);
    for (i = 0; i < ARRAY_SIZE(out_modes); i++)
    {
        SetConsoleMode(cout, saved_out);
        _snprintf(id, sizeof(id), "output.set-%s", out_modes[i].n);
        ok = SetConsoleMode(cout, out_modes[i].v);
        m = 0xdead;
        GetConsoleMode(cout, &m);
        wd_emit(id, "%s get=0x%lx", wd_res(ok), m);
    }
    SetConsoleMode(cout, saved_out);
    EMIT_B("GetConsoleMode-invalid-handle", GetConsoleMode(INVALID_HANDLE_VALUE, &m));
    EMIT_B("GetConsoleMode-null-handle", GetConsoleMode(NULL, &m));
    f = wd_open(L"plain.txt", GENERIC_READ | GENERIC_WRITE, WD_RWD, CREATE_ALWAYS, 0);
    EMIT_B("GetConsoleMode-file-handle", GetConsoleMode(f, &m));
    EMIT_B("SetConsoleMode-file-handle", SetConsoleMode(f, 0));
    CLOSE(f);
    EMIT_B("GetConsoleMode-process-handle", GetConsoleMode(GetCurrentProcess(), &m));

    wd_emit("cp.default", "in=%u out=%u", GetConsoleCP(), GetConsoleOutputCP());
    for (i = 0; i < ARRAY_SIZE(cps); i++)
    {
        _snprintf(id, sizeof(id), "cp.set-output-%u", cps[i]);
        ok = SetConsoleOutputCP(cps[i]);
        wd_emit(id, "%s get=%u", wd_res(ok), GetConsoleOutputCP());
        _snprintf(id, sizeof(id), "cp.set-input-%u", cps[i]);
        ok = SetConsoleCP(cps[i]);
        wd_emit(id, "%s get=%u", wd_res(ok), GetConsoleCP());
    }
}

static void c_screen(void)
{
    CONSOLE_SCREEN_BUFFER_INFO info;
    CONSOLE_SCREEN_BUFFER_INFOEX infoex;
    CONSOLE_CURSOR_INFO ci;
    COORD c;
    SMALL_RECT r;
    CHAR_INFO fillc;
    DWORD n, m;
    WORD attrs[8];
    WCHAR line[200], title[300];
    BOOL ok;
    int i;
    HANDLE sb;

    ok = GetConsoleScreenBufferInfo(cout, &info);
    if (!ok) wd_emit("info", "e=%lu", GetLastError());
    else wd_emit("info", "size=%dx%d cursor=%d,%d attr=0x%x window=%d,%d-%d,%d max=%dx%d", info.dwSize.X, info.dwSize.Y,
                 info.dwCursorPosition.X, info.dwCursorPosition.Y, info.wAttributes, info.srWindow.Left, info.srWindow.Top,
                 info.srWindow.Right, info.srWindow.Bottom, info.dwMaximumWindowSize.X, info.dwMaximumWindowSize.Y);
    memset(&infoex, 0, sizeof(infoex));
    infoex.cbSize = sizeof(infoex);
    ok = GetConsoleScreenBufferInfoEx(cout, &infoex);
    wd_emit("infoex", ok ? "ok popup=0x%x fullscreen=%d color0=%06lx color7=%06lx color15=%06lx" : "e=%lu",
            ok ? (unsigned)infoex.wPopupAttributes : (unsigned)GetLastError(), infoex.bFullscreenSupported,
            infoex.ColorTable[0], infoex.ColorTable[7], infoex.ColorTable[15]);
    infoex.cbSize = 10;
    EMIT_B("infoex-bad-size", GetConsoleScreenBufferInfoEx(cout, &infoex));
    c = GetLargestConsoleWindowSize(cout);
    wd_emit("largest-window-nonzero", "%d", c.X > 0 && c.Y > 0);
    ok = GetConsoleCursorInfo(cout, &ci);
    wd_emit("cursor-info", ok ? "size=%lu visible=%d" : "e=%lu", ok ? ci.dwSize : GetLastError(), ci.bVisible);
    ci.dwSize = 0; ci.bVisible = TRUE;
    EMIT_B("cursor-info-size-0", SetConsoleCursorInfo(cout, &ci));
    ci.dwSize = 101;
    EMIT_B("cursor-info-size-101", SetConsoleCursorInfo(cout, &ci));
    ci.dwSize = 100; ci.bVisible = FALSE;
    EMIT_B("cursor-info-size-100-hidden", SetConsoleCursorInfo(cout, &ci));

    home();
    wr(L"abc");
    wd_emit("write.abc", "%s", cur());
    wr(L"\r\n");
    wd_emit("write.crlf", "%s", cur());
    wr(L"x\ty");
    wd_emit("write.tab", "%s", cur());
    wr(L"\b\b");
    wd_emit("write.backspace", "%s", cur());
    wr(L"\n");
    wd_emit("write.lf-only", "%s", cur());
    wr(L"\r\b");
    wd_emit("write.backspace-at-column-0", "%s", cur());
    wr(L"q\a");
    wd_emit("write.bell", "%s", cur());
    wd_emit("write.row0", "%s", row(0, 80));
    wd_emit("write.row1", "%s", row(1, 80));
    home();
    for (i = 0; i < 80; i++) line[i] = (WCHAR)('0' + i % 10);
    line[80] = 0;
    wr(line);
    wd_emit("wrap.after-80-characters", "%s", cur());
    wr(L"Z");
    wd_emit("wrap.after-81-characters", "%s", cur());
    wd_emit("wrap.row1", "%s", row(1, 80));
    home();
    c.X = 79; c.Y = 0;
    SetConsoleCursorPosition(cout, c);
    wr(L"\t");
    wd_emit("wrap.tab-at-last-column", "%s", cur());
    home();
    GetConsoleMode(cout, &m);
    SetConsoleMode(cout, m & ~ENABLE_WRAP_AT_EOL_OUTPUT);
    wr(line); wr(L"Z");
    wd_emit("nowrap.after-81-characters", "%s", cur());
    c.X = 79; c.Y = 0;
    line[0] = '?'; n = 0;
    ReadConsoleOutputCharacterW(cout, line, 1, c, &n);
    wd_emit("nowrap.last-cell", "%c", n ? (char)line[0] : '?');
    for (i = 0; i < 80; i++) line[i] = (WCHAR)('0' + i % 10);
    line[80] = 0;
    SetConsoleMode(cout, m & ~ENABLE_PROCESSED_OUTPUT);
    home();
    wr(L"a\r\n\tb");
    wd_emit("raw.control-characters-take-cells", "%s", cur());
    SetConsoleMode(cout, m);
    home();
    wr(L"\x4e2d\x6587");
    wd_emit("wide.two-cjk-characters", "%s", cur());
    home();
    wr(L"e\x0301");
    wd_emit("wide.combining-accent", "%s", cur());
    home();
    wr(L"\xd83d\xde00");
    wd_emit("wide.emoji-surrogate-pair", "%s", cur());
    home();
    for (i = 0; i < 30; i++)
    {
        _snwprintf(line, 100, L"line %d\r\n", i);
        wr(line);
    }
    wd_emit("scroll.cursor-after-30-lines", "%s", cur());
    wd_emit("scroll.first-row", "%s", row(0, 20));
    wd_emit("scroll.last-text-row", "%s", row(23, 20));

    home();
    c.X = 100; c.Y = 5;
    EMIT_B("cursor.x-out-of-range", SetConsoleCursorPosition(cout, c));
    c.X = 5; c.Y = 25;
    EMIT_B("cursor.y-out-of-range", SetConsoleCursorPosition(cout, c));
    c.X = -1; c.Y = 0;
    EMIT_B("cursor.negative", SetConsoleCursorPosition(cout, c));
    c.X = 79; c.Y = 24;
    EMIT_B("cursor.last-cell", SetConsoleCursorPosition(cout, c));
    wr(L"L");
    wd_emit("cursor.write-at-last-cell", "%s row23=[%s]", cur(), row(23, 80));

    home();
    c.X = 78; c.Y = 0;
    n = 99; ok = WriteConsoleOutputCharacterW(cout, L"abcdef", 6, c, &n);
    wd_emit("output-character.runs-onto-next-row", "%s n=%lu row1=[%s]", wd_res(ok), n, row(1, 10));
    c.X = 78; c.Y = 24;
    n = 99; ok = WriteConsoleOutputCharacterW(cout, L"abcdef", 6, c, &n);
    wd_emit("output-character.runs-off-the-end", "%s n=%lu", wd_res(ok), n);
    c.X = 0; c.Y = 30;
    n = 99; ok = WriteConsoleOutputCharacterW(cout, L"abc", 3, c, &n);
    wd_emit("output-character.outside-buffer", "%s n=%lu", wd_res(ok), n);
    n = 99; ok = ReadConsoleOutputCharacterW(cout, line, 10, c, &n);
    wd_emit("read-character.outside-buffer", "%s n=%lu", wd_res(ok), n);
    c.X = 75; c.Y = 24;
    n = 99; ok = ReadConsoleOutputCharacterW(cout, line, 10, c, &n);
    wd_emit("read-character.runs-off-the-end", "%s n=%lu", wd_res(ok), n);
    c.X = 0; c.Y = 2;
    n = 99; ok = FillConsoleOutputCharacterW(cout, '#', 100, c, &n);
    wd_emit("fill.100-cells", "%s n=%lu row3=[%s]", wd_res(ok), n, row(3, 30));
    n = 99; ok = FillConsoleOutputAttribute(cout, 0x4e, 3, c, &n);
    wd_emit("fill.attribute", "%s n=%lu", wd_res(ok), n);
    memset(attrs, 0, sizeof(attrs));
    n = 99; ok = ReadConsoleOutputAttribute(cout, attrs, 5, c, &n);
    wd_emit("read-attribute", "%s n=%lu %x %x %x %x %x", wd_res(ok), n, attrs[0], attrs[1], attrs[2], attrs[3], attrs[4]);
    c.X = 0; c.Y = 24;
    n = 99; ok = FillConsoleOutputCharacterW(cout, '#', 200, c, &n);
    wd_emit("fill.past-the-end", "%s n=%lu", wd_res(ok), n);

    home();
    EMIT_B("attribute.set", SetConsoleTextAttribute(cout, 0x1c));
    wr(L"A");
    c.X = 0; c.Y = 0;
    ReadConsoleOutputAttribute(cout, attrs, 2, c, &n);
    wd_emit("attribute.of-written-cell", "%x next=%x", attrs[0], attrs[1]);
    EMIT_B("attribute.set-high-bits", SetConsoleTextAttribute(cout, 0xff07));
    GetConsoleScreenBufferInfo(cout, &info);
    wd_emit("attribute.high-bits-kept", "0x%x", info.wAttributes);
    EMIT_B("attribute.set-0", SetConsoleTextAttribute(cout, 0));
    SetConsoleTextAttribute(cout, 7);

    home();
    wr(L"0123456789");
    r.Left = 0; r.Top = 0; r.Right = 4; r.Bottom = 0;
    c.X = 20; c.Y = 5;
    fillc.Char.UnicodeChar = '.'; fillc.Attributes = 7;
    EMIT_B("scroll.ScrollConsoleScreenBuffer", ScrollConsoleScreenBufferW(cout, &r, NULL, c, &fillc));
    wd_emit("scroll.source-row", "%s", row(0, 20));
    wd_emit("scroll.target-row", "%s", row(5, 30));
    r.Left = 70; r.Top = 0; r.Right = 90; r.Bottom = 30;
    c.X = -5; c.Y = -5;
    EMIT_B("scroll.rectangle-outside", ScrollConsoleScreenBufferW(cout, &r, NULL, c, &fillc));

    /* sizes */
    c.X = 40; c.Y = 10;
    EMIT_B("size.smaller-than-window", SetConsoleScreenBufferSize(cout, c));
    c.X = 100; c.Y = 50;
    EMIT_B("size.larger", SetConsoleScreenBufferSize(cout, c));
    GetConsoleScreenBufferInfo(cout, &info);
    wd_emit("size.after-larger", "size=%dx%d window=%d,%d-%d,%d", info.dwSize.X, info.dwSize.Y, info.srWindow.Left,
            info.srWindow.Top, info.srWindow.Right, info.srWindow.Bottom);
    r.Left = 0; r.Top = 0; r.Right = 199; r.Bottom = 99;
    EMIT_B("window.larger-than-buffer", SetConsoleWindowInfo(cout, TRUE, &r));
    r.Left = 10; r.Top = 10; r.Right = 5; r.Bottom = 5;
    EMIT_B("window.inverted", SetConsoleWindowInfo(cout, TRUE, &r));
    r.Left = 0; r.Top = 0; r.Right = 39; r.Bottom = 9;
    EMIT_B("window.smaller", SetConsoleWindowInfo(cout, TRUE, &r));
    r.Left = 1; r.Top = 1; r.Right = 1; r.Bottom = 1;
    EMIT_B("window.relative-move", SetConsoleWindowInfo(cout, FALSE, &r));
    GetConsoleScreenBufferInfo(cout, &info);
    wd_emit("window.after-relative-move", "window=%d,%d-%d,%d", info.srWindow.Left, info.srWindow.Top, info.srWindow.Right, info.srWindow.Bottom);
    c.X = 0; c.Y = 0;
    EMIT_B("size.zero", SetConsoleScreenBufferSize(cout, c));
    c.X = 32767; c.Y = 32767;
    EMIT_B("size.huge", SetConsoleScreenBufferSize(cout, c));

    /* a second screen buffer */
    sb = CreateConsoleScreenBuffer(GENERIC_READ | GENERIC_WRITE, WD_RW, NULL, CONSOLE_TEXTMODE_BUFFER, NULL);
    EMIT_B("second-buffer.create", sb != INVALID_HANDLE_VALUE);
    ok = GetConsoleScreenBufferInfo(sb, &info);
    wd_emit("second-buffer.info", ok ? "size=%dx%d cursor=%d,%d attr=0x%x" : "e=%lu", ok ? info.dwSize.X : (int)GetLastError(),
            info.dwSize.Y, info.dwCursorPosition.X, info.dwCursorPosition.Y, info.wAttributes);
    n = 0; ok = WriteConsoleW(sb, L"hidden", 6, &n, NULL);
    wd_emit("second-buffer.write-while-inactive", "%s n=%lu", wd_res(ok), n);
    EMIT_B("second-buffer.activate", SetConsoleActiveScreenBuffer(sb));
    EMIT_B("second-buffer.activate-input-handle", SetConsoleActiveScreenBuffer(cin));
    EMIT_B("second-buffer.reactivate-first", SetConsoleActiveScreenBuffer(cout));
    CloseHandle(sb);
    sb = CreateConsoleScreenBuffer(GENERIC_READ | GENERIC_WRITE, WD_RW, NULL, 2, NULL);
    EMIT_B("second-buffer.create-bad-type", sb != INVALID_HANDLE_VALUE);
    if (sb != INVALID_HANDLE_VALUE) CloseHandle(sb);

    /* the title */
    EMIT_B("title.set", SetConsoleTitleW(L"windiff title"));
    title[0] = 0;
    n = GetConsoleTitleW(title, 290);
    wd_emit("title.get", "%lu [%s]", n, wd_u8(title));
    title[0] = 'X'; title[1] = 'Y'; title[2] = 0;
    n = GetConsoleTitleW(title, 5);
    wd_emit("title.get-small-buffer", "%lu [%s]", n, wd_u8(title));
    n = GetConsoleTitleW(title, 0);
    wd_emit("title.get-zero-buffer", "%lu", n);
    EMIT_B("title.set-empty", SetConsoleTitleW(L""));
    SetLastError(0xdeadbeef);
    n = GetConsoleTitleW(title, 290);
    wd_emit("title.get-empty", "%lu gle=%lu", n, GetLastError());
}

static void c_vt(void)
{
    DWORD m, n;
    WORD attrs[4];
    COORD c = {0, 0};
    BOOL ok;

    GetConsoleMode(cout, &m);
    home();
    wr(L"\x1b[5;10Hx");
    wd_emit("off.cursor-sequence-is-printed", "%s", cur());
    ok = SetConsoleMode(cout, m | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
    wd_emit("on.enable", "%s", wd_res(ok));
    home();
    wr(L"\x1b[5;10H");
    wd_emit("on.cursor-position", "%s", cur());
    wr(L"\x1b[2A");
    wd_emit("on.cursor-up-2", "%s", cur());
    wr(L"\x1b[100C");
    wd_emit("on.cursor-forward-100", "%s", cur());
    wr(L"\x1b[H");
    wd_emit("on.cursor-home", "%s", cur());
    wr(L"\x1b[999;999H");
    wd_emit("on.cursor-far", "%s", cur());
    wr(L"\x1b[1;1Habc\x1b[2;1Hdef\x1b[1;1H\x1b[K");
    wd_emit("on.erase-line", "row0=[%s] row1=[%s]", row(0, 10), row(1, 10));
    wr(L"\x1b[2J");
    wd_emit("on.erase-display", "row1=[%s] cursor=%s", row(1, 10), cur());
    wr(L"\x1b[1;1H\x1b[31mR\x1b[1mB\x1b[0mN\x1b[7mI\x1b[0m");
    memset(attrs, 0, sizeof(attrs));
    ReadConsoleOutputAttribute(cout, attrs, 4, c, &n);
    wd_emit("on.colours", "%x %x %x %x", attrs[0], attrs[1], attrs[2], attrs[3]);
    wr(L"\x1b[1;1H\x1b[s\x1b[10;10H\x1b[u");
    wd_emit("on.save-restore-cursor", "%s", cur());
    wr(L"\x1b[1;1H0123456789\x1b[1;3H\x1b[2P");
    wd_emit("on.delete-characters", "%s", row(0, 12));
    wr(L"\x1b[1;3H\x1b[2@");
    wd_emit("on.insert-characters", "%s", row(0, 12));
    wr(L"\x1b[1;1H\x1b[3X");
    wd_emit("on.erase-characters", "[%s]", row(0, 12));
    wr(L"\x1b[1;1Hab\x1b[1;1H\x1b[4hX\x1b[4l");
    wd_emit("on.insert-mode", "%s", row(0, 12));
    wr(L"\x1b[2J\x1b[1;1H\x1b[?25l");
    {
        CONSOLE_CURSOR_INFO ci;
        ci.bVisible = 7;
        GetConsoleCursorInfo(cout, &ci);
        wd_emit("on.hide-cursor", "visible=%d", ci.bVisible);
        wr(L"\x1b[?25h");
    }
    wr(L"\x1b[5;1r\x1b[5;1HA\nB\nC\x1b[r");
    wd_emit("on.scroll-margins", "%s", cur());
    wr(L"\x1b[2J\x1b[1;1H");
    wr(L"\x1b]0;vt title\x07");
    {
        WCHAR title[100];
        title[0] = 0;
        GetConsoleTitleW(title, 90);
        wd_emit("on.title-sequence", "[%s]", wd_u8(title));
    }
    wr(L"\x1b[1;1Ha\x1b[1;80Hb");
    wr(L"c");
    wd_emit("on.delayed-wrap-at-last-column", "%s", cur());
    wr(L"\x1b[2J\x1b[1;1H\x1b(0lqk\x1b(B");
    wd_emit("on.line-drawing-characters", "%s", row(0, 5));
    wr(L"\x1b[2J\x1b[1;1H\x1b[38;2;255;0;0mT\x1b[0m");
    ReadConsoleOutputAttribute(cout, attrs, 1, c, &n);
    wd_emit("on.true-colour-attribute", "%x", attrs[0]);
    wr(L"\x1b[1;1H\x1b[6n");
    {
        INPUT_RECORD rec[16];
        char reply[40];
        int len = 0;
        unsigned i;
        n = 0;
        Sleep(200);
        if (GetNumberOfConsoleInputEvents(cin, &n) && n)
        {
            if (n > 16) n = 16;
            ReadConsoleInputW(cin, rec, n, &n);
            for (i = 0; i < n && len < 30; i++)
                if (rec[i].EventType == KEY_EVENT && rec[i].Event.KeyEvent.bKeyDown)
                    len += _snprintf(reply + len, 8, rec[i].Event.KeyEvent.uChar.UnicodeChar == 0x1b ? "ESC" : "%c",
                                     (char)rec[i].Event.KeyEvent.uChar.UnicodeChar);
        }
        reply[len] = 0;
        wd_emit("on.device-status-report-reply", "%s", len ? reply : "(none)");
    }
    SetConsoleMode(cout, m);
}

static void key(INPUT_RECORD *r, WCHAR ch, WORD vk, BOOL down, DWORD ctrl)
{
    memset(r, 0, sizeof(*r));
    r->EventType = KEY_EVENT;
    r->Event.KeyEvent.bKeyDown = down;
    r->Event.KeyEvent.wRepeatCount = 1;
    r->Event.KeyEvent.wVirtualKeyCode = vk;
    r->Event.KeyEvent.uChar.UnicodeChar = ch;
    r->Event.KeyEvent.dwControlKeyState = ctrl;
}

static BOOL type(const WCHAR *s)
{
    INPUT_RECORD rec[64];
    DWORD n = 0, i;
    for (i = 0; s[i] && n < 62; i++)
    {
        WORD vk = s[i] == '\r' ? VK_RETURN : s[i] == 0x1a ? 'Z' : s[i] == 3 ? 'C' : (WORD)towupper(s[i]);
        DWORD ctrl = (s[i] == 0x1a || s[i] == 3) ? LEFT_CTRL_PRESSED : 0;
        key(&rec[n++], s[i], vk, TRUE, ctrl);
        key(&rec[n++], s[i], vk, FALSE, ctrl);
    }
    return WriteConsoleInputW(cin, rec, n, &i);
}

/* Reads only when input is ready, so that a console that never delivers
 * the line shows as "nothing to read" rather than as a hang. */
static const char *read_line(BOOL use_readfile)
{
    static char out[200];
    WCHAR wbuf[64];
    char abuf[64];
    DWORD n = 0, i;
    int len = 0;
    BOOL ok;

    if (WaitForSingleObject(cin, 3000) != WAIT_OBJECT_0) return "nothing to read";
    if (use_readfile)
    {
        ok = ReadFile(cin, abuf, 60, &n, NULL);
        if (!ok) { _snprintf(out, sizeof(out), "e=%lu", GetLastError()); return out; }
        len = _snprintf(out, 32, "n=%lu ", n);
        for (i = 0; i < n; i++) len += _snprintf(out + len, 8, "%02x ", (unsigned char)abuf[i]);
    }
    else
    {
        ok = ReadConsoleW(cin, wbuf, 60, &n, NULL);
        if (!ok) { _snprintf(out, sizeof(out), "e=%lu", GetLastError()); return out; }
        len = _snprintf(out, 32, "n=%lu ", n);
        for (i = 0; i < n; i++) len += _snprintf(out + len, 8, "%04x ", wbuf[i]);
    }
    out[len] = 0;
    return out;
}

static void c_input(void)
{
    INPUT_RECORD rec[16];
    DWORD n, m, saved;
    BOOL ok;
    unsigned i;
    char list[300];
    int len;

    GetConsoleMode(cin, &saved);
    EMIT_B("flush", FlushConsoleInputBuffer(cin));
    n = 99; ok = GetNumberOfConsoleInputEvents(cin, &n);
    wd_emit("events-after-flush", "%s n=%lu", wd_res(ok), n);
    wd_emit("wait-with-no-input", "%lu", WaitForSingleObject(cin, 0));
    n = 99; ok = PeekConsoleInputW(cin, rec, 16, &n);
    wd_emit("peek-with-no-input", "%s n=%lu", wd_res(ok), n);
    EMIT_B("write-input", type(L"ab"));
    n = 99; GetNumberOfConsoleInputEvents(cin, &n);
    wd_emit("events-after-two-keys", "%lu", n);
    wd_emit("wait-with-input", "%lu", WaitForSingleObject(cin, 0));
    n = 0; ok = PeekConsoleInputW(cin, rec, 16, &n);
    len = 0;
    for (i = 0; i < n && i < 16; i++)
        len += _snprintf(list + len, 16, "%c%c ", rec[i].Event.KeyEvent.bKeyDown ? 'd' : 'u', (char)rec[i].Event.KeyEvent.uChar.UnicodeChar);
    list[len] = 0;
    wd_emit("peek", "%s n=%lu %s", wd_res(ok), n, list);
    n = 0; ok = ReadConsoleInputW(cin, rec, 1, &n);
    wd_emit("read-one-record", "%s n=%lu", wd_res(ok), n);
    n = 99; GetNumberOfConsoleInputEvents(cin, &n);
    wd_emit("events-after-reading-one", "%lu", n);
    FlushConsoleInputBuffer(cin);

    /* cooked mode: a line */
    SetConsoleMode(cin, ENABLE_PROCESSED_INPUT | ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT);
    home();
    type(L"hi\r");
    wd_emit("line.ReadConsole", "%s", read_line(FALSE));
    wd_emit("line.echo-cursor", "%s", cur());
    wd_emit("line.echo-row", "%s", row(0, 10));
    type(L"xy\r");
    wd_emit("line.ReadFile", "%s", read_line(TRUE));
    type(L"abc\bd\r");
    wd_emit("line.backspace-edits", "%s", read_line(FALSE));
    type(L"\r");
    wd_emit("line.empty-line", "%s", read_line(FALSE));
    type(L"\x1a\r");
    wd_emit("line.ctrl-z-ReadFile", "%s", read_line(TRUE));
    type(L"a\x1a" L"b\r");
    wd_emit("line.ctrl-z-in-middle-ReadFile", "%s", read_line(TRUE));
    type(L"\x1a\r");
    wd_emit("line.ctrl-z-ReadConsole", "%s", read_line(FALSE));
    SetConsoleMode(cin, ENABLE_PROCESSED_INPUT | ENABLE_LINE_INPUT);
    home();
    type(L"no\r");
    wd_emit("line.no-echo", "%s cursor=%s", read_line(FALSE), cur());
    FlushConsoleInputBuffer(cin);

    /* raw mode: characters as they come */
    SetConsoleMode(cin, 0);
    type(L"r");
    wd_emit("raw.ReadConsole-one-key", "%s", read_line(FALSE));
    type(L"st");
    wd_emit("raw.ReadConsole-two-keys", "%s", read_line(FALSE));
    FlushConsoleInputBuffer(cin);
    type(L"\r");
    wd_emit("raw.enter", "%s", read_line(FALSE));
    type(L"\x03");
    wd_emit("raw.ctrl-c-is-a-character", "%s", read_line(FALSE));
    type(L"\x1a");
    wd_emit("raw.ctrl-z-ReadFile", "%s", read_line(TRUE));
    FlushConsoleInputBuffer(cin);

    /* records that are not keys */
    memset(rec, 0, sizeof(rec));
    rec[0].EventType = MOUSE_EVENT; rec[0].Event.MouseEvent.dwButtonState = 1;
    rec[1].EventType = WINDOW_BUFFER_SIZE_EVENT; rec[1].Event.WindowBufferSizeEvent.dwSize.X = 80;
    rec[2].EventType = FOCUS_EVENT; rec[2].Event.FocusEvent.bSetFocus = TRUE;
    rec[3].EventType = MENU_EVENT;
    rec[4].EventType = 0x40; /* not an event type */
    n = 99; ok = WriteConsoleInputW(cin, rec, 5, &n);
    wd_emit("other-records.write", "%s n=%lu", wd_res(ok), n);
    n = 0; ok = ReadConsoleInputW(cin, rec, 16, &n);
    len = 0;
    for (i = 0; i < n && i < 16; i++) len += _snprintf(list + len, 16, "0x%x ", rec[i].EventType);
    list[len] = 0;
    wd_emit("other-records.read-back", "%s n=%lu %s", wd_res(ok), n, list);
    FlushConsoleInputBuffer(cin);
    n = 99; ok = WriteConsoleInputW(cin, rec, 0, &n);
    wd_emit("write-zero-records", "%s n=%lu", wd_res(ok), n);
    n = 99; ok = WriteConsoleInputW(cout, rec, 1, &n);
    wd_emit("write-input-to-output-handle", "%s", wd_res(ok));
    n = 99; ok = ReadConsoleInputW(cout, rec, 1, &n);
    wd_emit("read-input-from-output-handle", "%s", wd_res(ok));
    EMIT_B("flush-output-handle", FlushConsoleInputBuffer(cout));
    n = 99; ok = GetNumberOfConsoleInputEvents(cout, &n);
    wd_emit("events-of-output-handle", "%s", wd_res(ok));
    n = 0; ok = GetNumberOfConsoleMouseButtons(&n);
    wd_emit("mouse-buttons", "%s", wd_res(ok));
    SetConsoleMode(cin, saved);
    (void)m;
}

static HANDLE ctrl_event;
static volatile LONG ctrl_type = -1;
static BOOL WINAPI on_ctrl(DWORD type_code)
{
    ctrl_type = (LONG)type_code;
    SetEvent(ctrl_event);
    return TRUE;
}

static void c_misc(void)
{
    HANDLE h, h2;
    DWORD n, list[8];
    char buf[32];
    WCHAR wbuf[64];
    BOOL ok;
    CONSOLE_FONT_INFO font;
    CONSOLE_HISTORY_INFO hist;
    CONSOLE_SELECTION_INFO sel;

    /* console handles as files */
    wd_emit("handle.GetFileType", "in=%lu out=%lu", GetFileType(cin), GetFileType(cout));
    SetLastError(0xdeadbeef);
    n = GetFileSize(cout, NULL);
    wd_emit("handle.GetFileSize", "%lx gle=%lu", n, GetLastError());
    SetLastError(0xdeadbeef);
    n = SetFilePointer(cout, 0, NULL, FILE_BEGIN);
    wd_emit("handle.SetFilePointer", "%lx gle=%lu", n, GetLastError());
    EMIT_B("handle.FlushFileBuffers-output", FlushFileBuffers(cout));
    EMIT_B("handle.FlushFileBuffers-input", FlushFileBuffers(cin));
    n = 99; ok = WriteFile(cin, "x", 1, &n, NULL);
    wd_emit("handle.WriteFile-to-input", "%s", wd_res(ok));
    n = 99; ok = ReadFile(cout, buf, 1, &n, NULL);
    wd_emit("handle.ReadFile-from-output", "%s", wd_res(ok));
    n = 99; ok = WriteFile(cout, "ok", 2, &n, NULL);
    wd_emit("handle.WriteFile-to-output", "%s n=%lu", wd_res(ok), n);
    n = 99; ok = WriteConsoleW(cout, L"", 0, &n, NULL);
    wd_emit("handle.WriteConsole-zero", "%s n=%lu", wd_res(ok), n);
    n = 99; ok = WriteConsoleW(cin, L"x", 1, &n, NULL);
    wd_emit("handle.WriteConsole-to-input", "%s", wd_res(ok));
    n = 99; ok = WriteConsoleW(INVALID_HANDLE_VALUE, L"x", 1, &n, NULL);
    wd_emit("handle.WriteConsole-invalid", "%s", wd_res(ok));
    h = wd_open(L"plain.txt", GENERIC_READ | GENERIC_WRITE, WD_RWD, CREATE_ALWAYS, 0);
    n = 99; ok = WriteConsoleW(h, L"x", 1, &n, NULL);
    wd_emit("handle.WriteConsole-to-file", "%s", wd_res(ok));
    n = 99; ok = ReadConsoleW(h, wbuf, 1, &n, NULL);
    wd_emit("handle.ReadConsole-from-file", "%s", wd_res(ok));
    CLOSE(h);

    /* opening the console by name */
    h = wd_open(L"CONOUT$", GENERIC_READ | GENERIC_WRITE, WD_RW, OPEN_EXISTING, 0);
    EMIT_H("open.CONOUT", h); CLOSE(h);
    h = wd_open(L"conout$", GENERIC_WRITE, 0, OPEN_EXISTING, 0);
    EMIT_H("open.conout-lower-write-only-no-share", h);
    ok = GetConsoleMode(h, &n);
    wd_emit("open.GetConsoleMode-write-only-handle", "%s", wd_res(ok));
    CLOSE(h);
    h = wd_open(L"CONOUT$", 0, 0, OPEN_EXISTING, 0);
    EMIT_H("open.CONOUT-no-access", h); CLOSE(h);
    h = wd_open(L"CONOUT$", GENERIC_READ | GENERIC_WRITE, WD_RW, CREATE_NEW, 0);
    EMIT_H("open.CONOUT-create-new", h); CLOSE(h);
    h = wd_open(L"CONOUT$", GENERIC_READ | GENERIC_WRITE, WD_RW, TRUNCATE_EXISTING, 0);
    EMIT_H("open.CONOUT-truncate-existing", h); CLOSE(h);
    h = wd_open(L"CONIN$", GENERIC_READ | GENERIC_WRITE, WD_RW, OPEN_EXISTING, 0);
    EMIT_H("open.CONIN", h); CLOSE(h);
    h = wd_open(L"CON", GENERIC_WRITE, WD_RW, OPEN_EXISTING, 0);
    EMIT_H("open.CON-write", h);
    wd_emit("open.CON-write-is-output", "%s", wd_res(GetConsoleScreenBufferInfo(h, (CONSOLE_SCREEN_BUFFER_INFO *)wbuf)));
    CLOSE(h);
    h = wd_open(L"CON", GENERIC_READ, WD_RW, OPEN_EXISTING, 0);
    EMIT_H("open.CON-read", h);
    n = 0;
    wd_emit("open.CON-read-is-input", "%s", wd_res(GetNumberOfConsoleInputEvents(h, &n)));
    CLOSE(h);
    h = wd_open(L"CON", GENERIC_READ | GENERIC_WRITE, WD_RW, OPEN_EXISTING, 0);
    EMIT_H("open.CON-read-write", h); CLOSE(h);
    h = wd_open(L"\\\\.\\CONOUT$", GENERIC_READ | GENERIC_WRITE, WD_RW, OPEN_EXISTING, 0);
    EMIT_H("open.device-prefix-CONOUT", h); CLOSE(h);
    h = wd_open(L"C:\\wd\\CONOUT$", GENERIC_READ | GENERIC_WRITE, WD_RW, OPEN_EXISTING, 0);
    EMIT_H("open.CONOUT-in-a-directory", h); CLOSE(h);
    h = wd_open(L"CONERR$", GENERIC_READ | GENERIC_WRITE, WD_RW, OPEN_EXISTING, 0);
    EMIT_H("open.CONERR", h); CLOSE(h);
    ok = DuplicateHandle(GetCurrentProcess(), cout, GetCurrentProcess(), &h2, 0, FALSE, DUPLICATE_SAME_ACCESS);
    wd_emit("handle.DuplicateHandle", "%s", wd_res(ok));
    if (ok)
    {
        wd_emit("handle.duplicate-is-console", "%s", wd_res(GetConsoleMode(h2, &n)));
        EMIT_B("handle.CloseHandle-duplicate", CloseHandle(h2));
    }
    EMIT_B("handle.SetStdHandle", SetStdHandle(STD_OUTPUT_HANDLE, cout));
    wd_emit("handle.GetStdHandle-after-set", "%d", GetStdHandle(STD_OUTPUT_HANDLE) == cout);
    SetLastError(0xdeadbeef);
    h = GetStdHandle(12345);
    wd_emit("handle.GetStdHandle-bad-id", "%s gle=%lu", h == INVALID_HANDLE_VALUE ? "invalid" : h ? "other" : "null", GetLastError());
    EMIT_B("handle.SetStdHandle-bad-id", SetStdHandle(12345, cout));

    /* the console itself */
    memset(list, 0, sizeof(list));
    n = GetConsoleProcessList(list, 8);
    wd_emit("process-list.count", "%lu first-is-self=%d", n, n && list[0] == GetCurrentProcessId());
    n = GetConsoleProcessList(list, 0);
    wd_emit("process-list.zero-buffer", "%lu gle=%lu", n, n ? 0 : GetLastError());
    EMIT_B("AttachConsole-while-attached", AttachConsole(ATTACH_PARENT_PROCESS));
    EMIT_B("AttachConsole-self", AttachConsole(GetCurrentProcessId()));
    EMIT_B("AttachConsole-bad-pid", AttachConsole(3));
    EMIT_B("AllocConsole-while-attached", AllocConsole());
    ok = GetCurrentConsoleFont(cout, FALSE, &font);
    wd_emit("font.GetCurrentConsoleFont", "%s", wd_res(ok));
    memset(&hist, 0, sizeof(hist)); hist.cbSize = sizeof(hist);
    ok = GetConsoleHistoryInfo(&hist);
    wd_emit("history.get", ok ? "ok size=%u buffers=%u flags=%lu" : "e=%lu", ok ? hist.HistoryBufferSize : (UINT)GetLastError(),
            hist.NumberOfHistoryBuffers, hist.dwFlags);
    hist.cbSize = 4;
    EMIT_B("history.bad-size", GetConsoleHistoryInfo(&hist));
    ok = GetConsoleSelectionInfo(&sel);
    wd_emit("selection", ok ? "ok flags=%lu" : "e=%lu", ok ? sel.dwFlags : GetLastError());
    n = 99; ok = GetConsoleDisplayMode(&n);
    wd_emit("display-mode", ok ? "ok %lu" : "e=%lu", ok ? n : GetLastError());
    EMIT_B("alias.add", AddConsoleAliasW((WCHAR *)L"wd", (WCHAR *)L"echo windiff", (WCHAR *)L"cmd.exe"));
    wbuf[0] = 0;
    n = GetConsoleAliasW((WCHAR *)L"wd", wbuf, sizeof(wbuf), (WCHAR *)L"cmd.exe");
    wd_emit("alias.get", "%lu [%s]", n, wd_u8(wbuf));
    n = GetConsoleAliasW((WCHAR *)L"nope", wbuf, sizeof(wbuf), (WCHAR *)L"cmd.exe");
    wd_emit("alias.get-missing", "%lu gle=%lu", n, GetLastError());

    /* control events */
    ctrl_event = CreateEventW(NULL, FALSE, FALSE, NULL);
    EMIT_B("ctrl.set-handler", SetConsoleCtrlHandler(on_ctrl, TRUE));
    EMIT_B("ctrl.generate-ctrl-c", GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0));
    n = WaitForSingleObject(ctrl_event, 3000);
    wd_emit("ctrl.ctrl-c-reaches-handler", n == WAIT_OBJECT_0 ? "type=%ld" : "not called", ctrl_type);
    ctrl_type = -1;
    EMIT_B("ctrl.generate-ctrl-break", GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, 0));
    n = WaitForSingleObject(ctrl_event, 3000);
    wd_emit("ctrl.ctrl-break-reaches-handler", n == WAIT_OBJECT_0 ? "type=%ld" : "not called", ctrl_type);
    ctrl_type = -1;
    EMIT_B("ctrl.generate-bad-event", GenerateConsoleCtrlEvent(5, 0));
    EMIT_B("ctrl.generate-to-bad-group", GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, 0x7ffffff0));
    EMIT_B("ctrl.ignore-ctrl-c", SetConsoleCtrlHandler(NULL, TRUE));
    GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0);
    n = WaitForSingleObject(ctrl_event, 1000);
    wd_emit("ctrl.ctrl-c-while-ignored", "%s", n == WAIT_OBJECT_0 ? "handler called" : "not called");
    EMIT_B("ctrl.stop-ignoring", SetConsoleCtrlHandler(NULL, FALSE));
    EMIT_B("ctrl.remove-handler", SetConsoleCtrlHandler(on_ctrl, FALSE));
    EMIT_B("ctrl.remove-handler-again", SetConsoleCtrlHandler(on_ctrl, FALSE));
    SetConsoleCtrlHandler(on_ctrl, TRUE);
    {
        /* processed input: the Ctrl+C key becomes the event, not a character */
        DWORD saved;
        GetConsoleMode(cin, &saved);
        SetConsoleMode(cin, ENABLE_PROCESSED_INPUT);
        FlushConsoleInputBuffer(cin);
        type(L"\x03");
        n = WaitForSingleObject(ctrl_event, 3000);
        wd_emit("ctrl.ctrl-c-key-in-processed-mode", n == WAIT_OBJECT_0 ? "event type=%ld" : "no event", ctrl_type);
        SetConsoleMode(cin, saved);
    }

    /* leaving the console */
    EMIT_B("free.FreeConsole", FreeConsole());
    wd_emit("free.code-pages-without-console", "in=%u out=%u", GetConsoleCP(), GetConsoleOutputCP());
    EMIT_B("free.GetConsoleMode-old-handle", GetConsoleMode(cout, &n));
    h = wd_open(L"CONOUT$", GENERIC_READ | GENERIC_WRITE, WD_RW, OPEN_EXISTING, 0);
    EMIT_H("free.open-CONOUT-without-console", h); CLOSE(h);
    EMIT_B("free.FreeConsole-again", FreeConsole());
    wd_emit("free.GetConsoleWindow", "%s", GetConsoleWindow() ? "window" : "null");
    n = GetConsoleProcessList(list, 8);
    wd_emit("free.process-list", "%lu", n);
}

static int child(int argc, WCHAR **argv)
{
    static const struct { const WCHAR *name; void (*fn)(void); } table[] = {
        {L"modes", c_modes}, {L"screen", c_screen}, {L"vt", c_vt}, {L"input", c_input}, {L"misc", c_misc},
    };
    unsigned i;

    if (argc < 5 || wcscmp(argv[2], L"pty")) return 2;
    wd_outh = CreateFileW(argv[4], GENERIC_WRITE, WD_RWD, NULL, CREATE_ALWAYS, 0, NULL);
    cin = wd_open(L"CONIN$", GENERIC_READ | GENERIC_WRITE, WD_RW, OPEN_EXISTING, 0);
    cout = wd_open(L"CONOUT$", GENERIC_READ | GENERIC_WRITE, WD_RW, OPEN_EXISTING, 0);
    wd_emit("pseudoconsole.child-opens-console", "in=%s out=%s", cin != INVALID_HANDLE_VALUE ? "ok" : "failed",
            cout != INVALID_HANDLE_VALUE ? "ok" : "failed");
    for (i = 0; i < ARRAY_SIZE(table); i++)
        if (!wcscmp(argv[3], table[i].name)) table[i].fn();
    return 0;
}

static const struct wd_group_def groups[] = {
    {"modes", run_pty}, {"screen", run_pty}, {"vt", run_pty}, {"input", run_pty}, {"misc", run_pty}, {NULL, NULL},
};

int wmain(int argc, WCHAR **argv)
{
    return wd_main("con", groups, argc, argv, child);
}
