/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * windiff: the child program proc.exe and con.exe start. What it does is
 * chosen by environment variables, so that its command line is free to be
 * the thing under test:
 *
 *   WD_OUT    file its check lines are appended to (else standard output)
 *   WD_EXE, WD_GROUP, WD_ID
 *             its check identifiers are WD_EXE.WD_GROUP.WD_ID.suffix
 *   WD_CWD    the parent's scratch directory (written as <CWD>)
 *   WD_MODE   argv | which | quiet | env | startup | exit:KIND
 *   WD_VARS   for env: names to report, separated by ';'
 */
#include "wd.h"
#include <shellapi.h>
#include <signal.h>

static char id_prefix[200], exe_name[40] = "child";
static volatile int *volatile null_ptr;
static WCHAR mode[200];

static void emit(const char *suffix, const char *value)
{
    char id[300];
    _snprintf(id, sizeof(id), "%s.%s", id_prefix, suffix);
    wd_emit(id, "%s", value);
}

static const char *join(int argc, WCHAR **argv)
{
    static char out[5000];
    int i, n = 0;
    n += _snprintf(out + n, 32, "%d:", argc);
    for (i = 0; i < argc && n < 4200; i++)
    {
        const char *a = wd_path(argv[i]);
        int l = (int)strlen(a);
        if (l > 300) n += _snprintf(out + n, 64, "[%d chars]", lstrlenW(argv[i]));
        else n += _snprintf(out + n, 400, "[%s]", a);
    }
    out[n] = 0;
    return out;
}

static DWORD WINAPI thread_exit_process(void *arg) { Sleep(300); ExitProcess((UINT)(ULONG_PTR)arg); return 0; }
static DWORD WINAPI thread_crash(void *arg) { volatile int *p = arg; *p = 1; return 0; }
static DWORD WINAPI thread_return(void *arg) { Sleep(300); return (DWORD)(ULONG_PTR)arg; }
static LONG WINAPI filter_execute(EXCEPTION_POINTERS *e) { (void)e; return EXCEPTION_EXECUTE_HANDLER; }
static LONG WINAPI filter_exit(EXCEPTION_POINTERS *e) { (void)e; ExitProcess(21); return 0; }
static void at_exit(void) { ExitProcess(22); }
static volatile int zero;
static int recurse(volatile int n) { volatile char pad[4096]; pad[0] = (char)n; return recurse(n + 1) + pad[0]; }

static int do_exit(const WCHAR *kind)
{
    volatile int *null = null_ptr;
    if (!wcscmp(kind, L"return0")) return 0;
    if (!wcscmp(kind, L"return77")) return 77;
    if (!wcscmp(kind, L"return-minus-1")) return -1;
    if (!wcscmp(kind, L"exit5")) exit(5);
    if (!wcscmp(kind, L"_exit6")) _exit(6);
    if (!wcscmp(kind, L"ExitProcess-big")) ExitProcess(0x12345678);
    if (!wcscmp(kind, L"ExitProcess-259")) ExitProcess(259);
    if (!wcscmp(kind, L"ExitProcess-ffffffff")) ExitProcess(0xffffffff);
    if (!wcscmp(kind, L"TerminateProcess-self")) TerminateProcess(GetCurrentProcess(), 9);
    if (!wcscmp(kind, L"abort")) abort();
    if (!wcscmp(kind, L"raise-SIGABRT")) raise(SIGABRT);
    if (!wcscmp(kind, L"raise-SIGSEGV")) raise(SIGSEGV);
    if (!wcscmp(kind, L"raise-SIGTERM")) raise(SIGTERM);
    if (!wcscmp(kind, L"raise-SIGINT")) raise(SIGINT);
    if (!wcscmp(kind, L"read-null")) return *null;
    if (!wcscmp(kind, L"write-null")) { *null = 1; return 1; }
    if (!wcscmp(kind, L"divide-by-zero")) return 10 / zero;
    if (!wcscmp(kind, L"illegal-instruction")) { __asm__ volatile("ud2"); return 1; }
    if (!wcscmp(kind, L"breakpoint")) { __asm__ volatile("int3"); return 1; }
    if (!wcscmp(kind, L"privileged-instruction")) { __asm__ volatile("hlt"); return 1; }
    if (!wcscmp(kind, L"fastfail")) { __asm__ volatile("int $0x29" : : "c"(7)); return 1; }
    if (!wcscmp(kind, L"DebugBreak")) { DebugBreak(); return 1; }
    if (!wcscmp(kind, L"stack-overflow")) return recurse(0);
    if (!wcscmp(kind, L"RaiseException")) { RaiseException(0xE0000042, 0, 0, NULL); return 1; }
    if (!wcscmp(kind, L"RaiseException-noncontinuable")) { RaiseException(0xE0000043, EXCEPTION_NONCONTINUABLE, 0, NULL); return 1; }
    if (!wcscmp(kind, L"RaiseException-success-code")) { RaiseException(0x00000042, 0, 0, NULL); return 1; }
    if (!wcscmp(kind, L"RaiseException-warning-code")) { RaiseException(0x80000042, 0, 0, NULL); return 1; }
    if (!wcscmp(kind, L"filter-execute-handler")) { SetUnhandledExceptionFilter(filter_execute); *null = 1; return 1; }
    if (!wcscmp(kind, L"filter-exits-21")) { SetUnhandledExceptionFilter(filter_exit); *null = 1; return 1; }
    if (!wcscmp(kind, L"atexit-exits-22")) { atexit(at_exit); return 1; }
    if (!wcscmp(kind, L"ExitThread-main-only")) ExitThread(11);
    if (!wcscmp(kind, L"ExitThread-main-other-exits-12"))
    {
        CreateThread(NULL, 0, thread_exit_process, (void *)12, 0, NULL);
        ExitThread(11);
    }
    if (!wcscmp(kind, L"ExitThread-main-other-returns-13"))
    {
        CreateThread(NULL, 0, thread_return, (void *)13, 0, NULL);
        ExitThread(11);
    }
    if (!wcscmp(kind, L"return-while-thread-runs"))
    {
        CreateThread(NULL, 0, thread_exit_process, (void *)12, 0, NULL);
        return 14;
    }
    if (!wcscmp(kind, L"crash-in-thread"))
    {
        HANDLE t = CreateThread(NULL, 0, thread_crash, NULL, 0, NULL);
        WaitForSingleObject(t, 5000);
        return 15;
    }
    if (!wcscmp(kind, L"TerminateThread-main"))
    {
        HANDLE main_thread;
        DuplicateHandle(GetCurrentProcess(), GetCurrentThread(), GetCurrentProcess(), &main_thread, 0, FALSE, DUPLICATE_SAME_ACCESS);
        TerminateThread(main_thread, 16);
        Sleep(1000);
        return 17;
    }
    if (!wcscmp(kind, L"FatalExit")) FatalExit(23);
    if (!wcscmp(kind, L"sleep")) { Sleep(20000); return 24; }
    return 99;
}

static const char *handle_kind(HANDLE h)
{
    static char b[4][64];
    static int i;
    char *p = b[i++ & 3];
    DWORD type, mode_bits;
    if (h == NULL) return "null";
    if (h == INVALID_HANDLE_VALUE) return "invalid";
    SetLastError(0);
    type = GetFileType(h);
    if (type == FILE_TYPE_UNKNOWN) { _snprintf(p, 64, "unknown(e=%lu)", GetLastError()); return p; }
    _snprintf(p, 64, "%s%s", type == FILE_TYPE_DISK ? "disk" : type == FILE_TYPE_CHAR ? "char" : type == FILE_TYPE_PIPE ? "pipe" : "other",
              GetConsoleMode(h, &mode_bits) ? "+console" : "");
    return p;
}

int wmain(int argc, WCHAR **argv)
{
    WCHAR out[600], group[100], idw[200], exe[40], vars[1000], cwd[600];
    char value[5000];
    int n, sargc = 0;
    WCHAR **sargv;

    SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
    wd_exe = exe_name;
    wd_outh = GetStdHandle(STD_OUTPUT_HANDLE);
    if (GetEnvironmentVariableW(L"WD_OUT", out, 590))
    {
        wd_outh = CreateFileW(out, FILE_APPEND_DATA, WD_RWD, NULL, OPEN_ALWAYS, 0, NULL);
        SetFilePointer(wd_outh, 0, NULL, FILE_END);
    }
    group[0] = 0; idw[0] = 0; mode[0] = 0; exe[0] = 0;
    if (GetEnvironmentVariableW(L"WD_EXE", exe, 38))
        WideCharToMultiByte(CP_UTF8, 0, exe, -1, exe_name, sizeof(exe_name) - 1, NULL, NULL);
    GetEnvironmentVariableW(L"WD_GROUP", group, 90);
    GetEnvironmentVariableW(L"WD_ID", idw, 190);
    GetEnvironmentVariableW(L"WD_MODE", mode, 190);
    WideCharToMultiByte(CP_UTF8, 0, group, -1, wd_group, sizeof(wd_group) - 1, NULL, NULL);
    WideCharToMultiByte(CP_UTF8, 0, idw, -1, id_prefix, sizeof(id_prefix) - 1, NULL, NULL);
    GetEnvironmentVariableW(L"WD_CWD", wd_cwd, ARRAY_SIZE(wd_cwd) - 1);
    GetModuleFileNameW(NULL, wd_self, ARRAY_SIZE(wd_self));

    if (!wcsncmp(mode, L"exit:", 5)) return do_exit(mode + 5);
    if (!wcscmp(mode, L"quiet")) return 42;
    if (!wcscmp(mode, L"which"))
    {
        emit("ran", wd_path(wd_self));
        return 42;
    }
    if (!wcscmp(mode, L"env"))
    {
        WCHAR *block = GetEnvironmentStringsW(), *p, *name, *next;
        int count = 0, sorted = 1, hidden = 0;
        const WCHAR *prev = NULL;
        n = 0;
        vars[0] = 0;
        GetEnvironmentVariableW(L"WD_VARS", vars, 990);
        for (name = vars; *name; name = next)
        {
            WCHAR val[300];
            DWORD r;
            next = wcschr(name, ';');
            if (next) *next++ = 0; else next = name + lstrlenW(name);
            SetLastError(0);
            r = GetEnvironmentVariableW(name, val, 290);
            if (!r && GetLastError()) n += _snprintf(value + n, 200, "%s=(unset) ", wd_u8(name));
            else n += _snprintf(value + n, 700, "%s=[%s] ", wd_u8(name), wd_path(val));
        }
        value[n] = 0;
        emit("vars", value);
        for (p = block; p && *p; p += lstrlenW(p) + 1)
        {
            if (*p == '=') { hidden++; continue; }
            if (!wcsncmp(p, L"WD_", 3)) continue;
            count++;
            if (prev && _wcsicmp(prev, p) > 0) sorted = 0;
            prev = p;
        }
        _snprintf(value, sizeof(value), "sorted=%d has-hidden-drive-variables=%d", sorted, hidden > 0);
        emit("block", value);
        return 42;
    }
    if (!wcscmp(mode, L"startup"))
    {
        STARTUPINFOW si;
        memset(&si, 0, sizeof(si));
        GetStartupInfoW(&si);
        _snprintf(value, sizeof(value), "flags=0x%lx show=%u title=[%s] desktop=[%s] x=%lu y=%lu cbReserved2=%u",
                  si.dwFlags, si.wShowWindow, wd_path(si.lpTitle), wd_u8(si.lpDesktop), si.dwX, si.dwY, si.cbReserved2 ? 1 : 0);
        emit("startupinfo", value);
        _snprintf(value, sizeof(value), "in=%s out=%s err=%s", handle_kind(GetStdHandle(STD_INPUT_HANDLE)),
                  handle_kind(GetStdHandle(STD_OUTPUT_HANDLE)), handle_kind(GetStdHandle(STD_ERROR_HANDLE)));
        emit("std-handles", value);
        _snprintf(value, sizeof(value), "window=%d cp=%u outcp=%u", GetConsoleWindow() != NULL, GetConsoleCP(), GetConsoleOutputCP());
        emit("console", value);
        cwd[0] = 0;
        GetCurrentDirectoryW(590, cwd);
        emit("cwd", wd_path(cwd));
        _snprintf(value, sizeof(value), "0x%lx", GetPriorityClass(GetCurrentProcess()));
        emit("priority-class", value);
        return 42;
    }

    /* argv: the command line three ways */
    emit("raw", wd_path(GetCommandLineW()));
    emit("crt", join(argc, argv));
    sargv = CommandLineToArgvW(GetCommandLineW(), &sargc);
    emit("shell", sargv ? join(sargc, sargv) : "failed");
    return 42;
}
