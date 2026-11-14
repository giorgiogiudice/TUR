/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * windiff: the small framework every check program includes.
 *
 * A check program is one .exe with several groups. `prog --list` prints the
 * group names; `prog --run GROUP` runs one group in the current directory
 * (the runner gives each group an empty scratch directory with the same
 * path on Windows and under Wine) and prints one JSON line per check:
 *
 *     {"id":"prog.group.check","v":"what was observed"}
 *
 * and a last line {"done":"prog.group"}. The value is whatever the public
 * API returned, written so that it is the same on two machines that behave
 * the same: error codes, counts, attribute bits, names; never handles,
 * addresses, times of day or process identifiers.
 *
 * Windows is only ever observed through its documented API.
 */
#ifndef WD_H
#define WD_H

#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0A00
#endif
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

#ifndef ARRAY_SIZE
#define ARRAY_SIZE(a) (sizeof(a) / sizeof((a)[0]))
#endif

#define WD_RW  (FILE_SHARE_READ | FILE_SHARE_WRITE)
#define WD_RWD (FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)

static HANDLE wd_outh;
static const char *wd_exe = "?";
static char wd_group[64] = "?";
static WCHAR wd_cwd[600];
static WCHAR wd_self[600];

static void wd_write(const char *s, int n)
{
    DWORD written;
    WriteFile(wd_outh, s, n, &written, NULL);
}

static void wd_emit(const char *check, const char *fmt, ...) __attribute__((format(printf, 2, 3)));
static void wd_emit(const char *check, const char *fmt, ...)
{
    static char val[6000], line[13000];
    int i, n;
    va_list ap;

    va_start(ap, fmt);
    _vsnprintf(val, sizeof(val) - 1, fmt, ap);
    va_end(ap);
    val[sizeof(val) - 1] = 0;
    n = _snprintf(line, 400, "{\"id\":\"%s.%s.%s\",\"v\":\"", wd_exe, wd_group, check);
    if (n < 0) n = 400;
    for (i = 0; val[i] && n < (int)sizeof(line) - 16; i++)
    {
        unsigned char c = (unsigned char)val[i];
        if (c == '"' || c == '\\') { line[n++] = '\\'; line[n++] = (char)c; }
        else if (c < 0x20 || c == 0x7f) n += _snprintf(line + n, 8, "\\u%04x", c);
        else line[n++] = (char)c;
    }
    line[n++] = '"'; line[n++] = '}'; line[n++] = '\n';
    wd_write(line, n);
}

/* UTF-8 copy of a wide string, in one of eight rotating buffers. */
static const char *wd_u8n(const WCHAR *w, int len)
{
    static char bufs[8][2600];
    static int idx;
    char *b = bufs[idx++ & 7];
    int n;

    if (!w) return "(null)";
    if (len < 0) len = lstrlenW(w);
    if (len > 800) len = 800;
    n = len ? WideCharToMultiByte(CP_UTF8, 0, w, len, b, 2590, NULL, NULL) : 0;
    b[n] = 0;
    return b;
}
static const char *wd_u8(const WCHAR *w) { return wd_u8n(w, -1); }

/* The same, with the scratch directory written as <CWD>. */
static const char *wd_path(const WCHAR *p)
{
    static WCHAR tmp[4][1300];
    static int idx;
    WCHAR *o = tmp[idx++ & 3];
    int cl = lstrlenW(wd_cwd), i, n = 0, pl;

    if (!p) return "(null)";
    pl = lstrlenW(p);
    for (i = 0; i < pl && n < 1200; )
    {
        if (cl && i + cl <= pl && !_wcsnicmp(p + i, wd_cwd, cl))
        {
            memcpy(o + n, L"<CWD>", 5 * sizeof(WCHAR));
            n += 5; i += cl;
        }
        else o[n++] = p[i++];
    }
    o[n] = 0;
    return wd_u8(o);
}

/* "ok", or the last error. Call it with the API call as its argument. */
static const char *wd_res(BOOL ok)
{
    static char b[8][32];
    static int i;
    char *p = b[i++ & 7];
    if (ok) return "ok";
    _snprintf(p, 32, "e=%lu", GetLastError());
    return p;
}
static const char *wd_hres(HANDLE h) { return wd_res(h != INVALID_HANDLE_VALUE && h != NULL); }

#define EMIT_B(id, ok) wd_emit(id, "%s", wd_res(ok))
#define EMIT_H(id, h)  wd_emit(id, "%s", wd_hres(h))
#define CLOSE(h) do { if ((h) != INVALID_HANDLE_VALUE && (h) != NULL) CloseHandle(h); (h) = INVALID_HANDLE_VALUE; } while (0)

static HANDLE wd_open(const WCHAR *name, DWORD access, DWORD share, DWORD disp, DWORD flags)
{
    return CreateFileW(name, access, share, NULL, disp, flags, NULL);
}

static BOOL wd_mkfile(const WCHAR *name, const char *data)
{
    DWORD n;
    HANDLE h = wd_open(name, GENERIC_WRITE, 0, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL);
    if (h == INVALID_HANDLE_VALUE) return FALSE;
    WriteFile(h, data, (DWORD)strlen(data), &n, NULL);
    CloseHandle(h);
    return TRUE;
}

/* "a=0x20" or "e=2". */
static const char *wd_attr(const WCHAR *name)
{
    static char b[8][32];
    static int i;
    char *p = b[i++ & 7];
    DWORD a = GetFileAttributesW(name);
    if (a == INVALID_FILE_ATTRIBUTES) _snprintf(p, 32, "e=%lu", GetLastError());
    else _snprintf(p, 32, "a=0x%lx", a);
    return p;
}

/* Sorted (by code unit) listing of a directory: "a|b/|c", "/" marks a
 * directory. "." and ".." are left out. */
static const char *wd_ls(const WCHAR *dir)
{
    static WCHAR names[96][264];
    static WCHAR joined[4096];
    WCHAR pattern[700];
    WIN32_FIND_DATAW fd;
    HANDLE h;
    int count = 0, i, j, n = 0;

    _snwprintf(pattern, 690, L"%ls\\*", dir);
    h = FindFirstFileW(pattern, &fd);
    if (h == INVALID_HANDLE_VALUE)
    {
        static char e[32];
        _snprintf(e, 32, "(e=%lu)", GetLastError());
        return e;
    }
    do
    {
        if (!lstrcmpW(fd.cFileName, L".") || !lstrcmpW(fd.cFileName, L"..")) continue;
        if (count >= 96) break;
        lstrcpynW(names[count], fd.cFileName, 260);
        if (fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) lstrcatW(names[count], L"/");
        count++;
    } while (FindNextFileW(h, &fd));
    FindClose(h);
    for (i = 1; i < count; i++)
        for (j = i; j > 0 && wcscmp(names[j - 1], names[j]) > 0; j--)
        {
            WCHAR t[264];
            memcpy(t, names[j], sizeof(t));
            memcpy(names[j], names[j - 1], sizeof(t));
            memcpy(names[j - 1], t, sizeof(t));
        }
    joined[0] = 0;
    for (i = 0; i < count && n < 3700; i++)
    {
        int l = lstrlenW(names[i]);
        if (i) joined[n++] = '|';
        memcpy(joined + n, names[i], l * sizeof(WCHAR));
        n += l;
    }
    joined[n] = 0;
    if (!count) return "(empty)";
    return wd_u8(joined);
}

static void wd_rmtree_full(WCHAR *path, int len)
{
    WIN32_FIND_DATAW fd;
    HANDLE h;

    if (len > 1900) return;
    lstrcpyW(path + len, L"\\*");
    h = FindFirstFileW(path, &fd);
    path[len] = 0;
    if (h == INVALID_HANDLE_VALUE) return;
    do
    {
        int n = lstrlenW(fd.cFileName);
        if (!lstrcmpW(fd.cFileName, L".") || !lstrcmpW(fd.cFileName, L"..")) continue;
        if (len + n + 4 >= 2000) continue;
        path[len] = '\\';
        memcpy(path + len + 1, fd.cFileName, (n + 1) * sizeof(WCHAR));
        if (fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY)
        {
            SetFileAttributesW(path, FILE_ATTRIBUTE_DIRECTORY);
            wd_rmtree_full(path, len + 1 + n);
            RemoveDirectoryW(path);
        }
        else
        {
            SetFileAttributesW(path, FILE_ATTRIBUTE_NORMAL);
            DeleteFileW(path);
        }
        path[len] = 0;
    } while (FindNextFileW(h, &fd));
    FindClose(h);
}

/* Empties the scratch directory, whatever the names in it. Only ever a
 * directory below one named "wd": the runner makes C:\wd\<prog>-<group>,
 * and a program started anywhere else must not delete what it finds. */
static BOOL wd_scratch_ok(void)
{
    return wcsstr(wd_cwd, L"\\wd\\") != NULL && lstrlenW(wd_cwd) > 6;
}

static void wd_clean(void)
{
    static WCHAR path[2100];
    if (!wd_scratch_ok()) return;
    SetCurrentDirectoryW(wd_cwd);
    lstrcpyW(path, L"\\\\?\\");
    lstrcatW(path, wd_cwd);
    wd_rmtree_full(path, lstrlenW(path));
}

/* Runs a child with our output as its standard output and waits for it.
 * Returns FALSE with the last error set when it could not be started; the
 * exit code is 0xfffffffe on a timeout (the child is then terminated). */
static BOOL wd_run_env(const WCHAR *app, const WCHAR *cmdline, DWORD flags, void *env, const WCHAR *dir,
                       DWORD timeout, DWORD *code)
{
    static WCHAR cmd[8192];
    STARTUPINFOW si;
    PROCESS_INFORMATION pi;

    memset(&si, 0, sizeof(si));
    si.cb = sizeof(si);
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    si.hStdOutput = wd_outh;
    si.hStdError = GetStdHandle(STD_ERROR_HANDLE);
    if (cmdline) lstrcpynW(cmd, cmdline, 8190);
    *code = 0xffffffff;
    if (!CreateProcessW(app, cmdline ? cmd : NULL, NULL, NULL, TRUE, flags, env, dir, &si, &pi)) return FALSE;
    if (WaitForSingleObject(pi.hProcess, timeout) != WAIT_OBJECT_0)
    {
        TerminateProcess(pi.hProcess, 0xfffffffe);
        WaitForSingleObject(pi.hProcess, 5000);
    }
    GetExitCodeProcess(pi.hProcess, code);
    CloseHandle(pi.hThread);
    CloseHandle(pi.hProcess);
    return TRUE;
}
static BOOL wd_run(const WCHAR *cmdline, DWORD *code)
{
    return wd_run_env(NULL, cmdline, 0, NULL, NULL, 30000, code);
}

struct wd_group_def { const char *name; void (*fn)(void); };

/* Called from wmain. `child` handles `prog --child MODE GROUP args...`
 * (argv[2] is the mode, argv[3] the group the lines belong to). */
static int wd_main(const char *exe, const struct wd_group_def *groups, int argc, WCHAR **argv,
                   int (*child)(int argc, WCHAR **argv))
{
    int i;

    wd_exe = exe;
    wd_outh = GetStdHandle(STD_OUTPUT_HANDLE);
    SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
    GetCurrentDirectoryW(ARRAY_SIZE(wd_cwd), wd_cwd);
    GetModuleFileNameW(NULL, wd_self, ARRAY_SIZE(wd_self));

    if (argc >= 2 && !wcscmp(argv[1], L"--list"))
    {
        for (i = 0; groups[i].name; i++)
        {
            wd_write(groups[i].name, (int)strlen(groups[i].name));
            wd_write("\n", 1);
        }
        return 0;
    }
    if (argc >= 4 && !wcscmp(argv[1], L"--child") && child)
    {
        WideCharToMultiByte(CP_UTF8, 0, argv[3], -1, wd_group, sizeof(wd_group) - 1, NULL, NULL);
        return child(argc, argv);
    }
    if (argc >= 3 && !wcscmp(argv[1], L"--run"))
    {
        if (!wd_scratch_ok())
        {
            wd_write("refusing to run outside a scratch directory below \\wd\\\n", 55);
            return 3;
        }
        WideCharToMultiByte(CP_UTF8, 0, argv[2], -1, wd_group, sizeof(wd_group) - 1, NULL, NULL);
        for (i = 0; groups[i].name; i++)
        {
            char done[160];
            if (strcmp(groups[i].name, wd_group)) continue;
            wd_clean();
            /* canaries: these differ only if the two machines were not set up alike */
            wd_emit("_env.cwd", "%s", wd_u8(wd_cwd));
            wd_emit("_env.self", "%s", wd_u8(wd_self));
            groups[i].fn();
            wd_clean();
            wd_write(done, _snprintf(done, sizeof(done), "{\"done\":\"%s.%s\"}\n", wd_exe, wd_group));
            return 0;
        }
    }
    wd_write("usage: --list | --run GROUP\n", 28);
    return 2;
}

#endif /* WD_H */
