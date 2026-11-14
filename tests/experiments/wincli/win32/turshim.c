// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// turshim.exe: a Windows command whose work is done by a Linux program.
//
// Copied under the name of a command (tracert.exe, winget.exe, ...) into a
// directory at the front of PATH inside the Windows environment, it starts
// the Linux helper named by TUR_WINSHIM with its own name and arguments, so
// that the command can be used from cmd.exe or PowerShell like any other:
// in a pipe, in a for loop, in a batch file.
//
// Wine can start a Linux program from a Windows one but hands back no
// process to wait for. So this program gives the helper a pipe as standard
// output, copies what arrives to its own standard output until the pipe
// closes, and takes the exit code from a trailer the helper appends
// (\x1e TURSHIM-RC=<n> \x1e) when TURSHIM_TRAILER is set.
//
// Built in CI with mingw-w64; a prototype, not shipped.

#include <windows.h>
#include <stdlib.h>
#include <string.h>

static WCHAR self[1024];
static WCHAR helper[2048];
static WCHAR cmdline[32768];

static void say(const char *text)
{
    DWORD done;
    WriteFile(GetStdHandle(STD_ERROR_HANDLE), text, (DWORD)strlen(text), &done, NULL);
}

static const WCHAR *skip_argv0(const WCHAR *p)
{
    if (*p == L'"')
    {
        p++;
        while (*p && *p != L'"') p++;
        if (*p) p++;
    }
    else
    {
        while (*p && *p != L' ' && *p != L'\t') p++;
    }
    while (*p == L' ' || *p == L'\t') p++;
    return p;
}

int main(void)
{
    SECURITY_ATTRIBUTES sa = { sizeof(sa), NULL, TRUE };
    STARTUPINFOW si;
    PROCESS_INFORMATION pi;
    HANDLE rd, wr, out = GetStdHandle(STD_OUTPUT_HANDLE);
    HANDLE in = GetStdHandle(STD_INPUT_HANDLE), err = GetStdHandle(STD_ERROR_HANDLE);
    WCHAR *name, *dot, *p;
    char *buf = NULL;
    size_t len = 0, cap = 0;
    DWORD got, done, code = 253;
    size_t i;

    if (!GetModuleFileNameW(NULL, self, 1024)) return 253;
    name = self;
    for (p = self; *p; p++) if (*p == L'\\' || *p == L'/') name = p + 1;
    dot = NULL;
    for (p = name; *p; p++) if (*p == L'.') dot = p;
    if (dot) *dot = 0;
    CharLowerW(name);

    got = GetEnvironmentVariableW(L"TUR_WINSHIM", helper, 2048);
    if (!got || got >= 2048)
    {
        say("turshim: TUR_WINSHIM is not set; this command is provided by TUR and needs its helper\r\n");
        return 253;
    }

    lstrcpyW(cmdline, L"\"");
    lstrcatW(cmdline, helper);
    lstrcatW(cmdline, L"\" ");
    lstrcatW(cmdline, name);
    lstrcatW(cmdline, L" ");
    p = (WCHAR *)skip_argv0(GetCommandLineW());
    if (lstrlenW(cmdline) + lstrlenW(p) + 1 >= 32768) return 253;
    lstrcatW(cmdline, p);

    if (!CreatePipe(&rd, &wr, &sa, 0)) return 253;
    SetHandleInformation(rd, HANDLE_FLAG_INHERIT, 0);
    SetHandleInformation(in, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
    SetHandleInformation(err, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
    SetEnvironmentVariableW(L"TURSHIM_TRAILER", L"1");

    ZeroMemory(&si, sizeof(si));
    si.cb = sizeof(si);
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdInput = in;
    si.hStdOutput = wr;
    si.hStdError = err;
    ZeroMemory(&pi, sizeof(pi));

    if (!CreateProcessW(helper, cmdline, NULL, NULL, TRUE, 0, NULL, NULL, &si, &pi))
    {
        say("turshim: could not start the TUR helper for this command\r\n");
        return 253;
    }
    CloseHandle(wr);

    for (;;)
    {
        if (cap - len < 65536)
        {
            cap = cap ? cap * 2 : 262144;
            buf = realloc(buf, cap);
            if (!buf) return 253;
        }
        if (!ReadFile(rd, buf + len, 65536, &got, NULL) || !got) break;
        len += got;
    }

    // The trailer: 0x1e "TURSHIM-RC=" digits 0x1e, at the very end.
    if (len >= 14 && buf[len - 1] == 0x1e)
    {
        for (i = len - 1; i > 0 && len - i < 40; i--)
        {
            if (buf[i - 1] == 0x1e && !memcmp(buf + i, "TURSHIM-RC=", 11))
            {
                code = strtoul(buf + i + 11, NULL, 10);
                len = i - 1;
                break;
            }
        }
    }
    else if (pi.hProcess)
    {
        WaitForSingleObject(pi.hProcess, INFINITE);
        GetExitCodeProcess(pi.hProcess, &code);
    }

    for (i = 0; i < len; i += done)
    {
        if (!WriteFile(out, buf + i, (DWORD)(len - i), &done, NULL) || !done) break;
    }
    return (int)code;
}
