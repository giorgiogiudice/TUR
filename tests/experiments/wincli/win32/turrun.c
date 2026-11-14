// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// turrun.exe: start a Windows command line exactly as given.
//
// Started from Linux, Wine rebuilds a command line from argv and quotes it
// its own way, which changes what cmd.exe sees. This launcher takes the
// command line from the environment instead (TURRUN_CMDLINE), so the test
// harness can hand cmd.exe the same characters a Windows console would.
//
//   TURRUN_CMDLINE       the command line to start
//   TURRUN_RC            a file to write the 32-bit exit code to (a Linux
//                        exit status keeps only the low eight bits)
//   TURRUN_PATH_PREPEND  directories to put in front of PATH first
//
// Built in CI with mingw-w64; test tooling, not shipped.

#include <windows.h>
#include <stdio.h>

static WCHAR cmdline[32768];
static WCHAR path[32768];
static WCHAR extra[8192];
static WCHAR joined[32768 + 8192];
static WCHAR rcfile[1024];

int main(void)
{
    STARTUPINFOW si;
    PROCESS_INFORMATION pi;
    DWORD code = 252, n;
    HANDLE in = GetStdHandle(STD_INPUT_HANDLE);
    HANDLE out = GetStdHandle(STD_OUTPUT_HANDLE);
    HANDLE err = GetStdHandle(STD_ERROR_HANDLE);

    n = GetEnvironmentVariableW(L"TURRUN_CMDLINE", cmdline, 32768);
    if (!n || n >= 32768) return 250;
    SetEnvironmentVariableW(L"TURRUN_CMDLINE", NULL);

    n = GetEnvironmentVariableW(L"TURRUN_RC", rcfile, 1024);
    if (n >= 1024) n = 0;
    rcfile[n] = 0;
    SetEnvironmentVariableW(L"TURRUN_RC", NULL);

    n = GetEnvironmentVariableW(L"TURRUN_PATH_PREPEND", extra, 8192);
    if (n && n < 8192)
    {
        DWORD m = GetEnvironmentVariableW(L"PATH", path, 32768);
        if (m >= 32768) m = 0;
        path[m] = 0;
        lstrcpyW(joined, extra);
        lstrcatW(joined, L";");
        lstrcatW(joined, path);
        SetEnvironmentVariableW(L"PATH", joined);
    }
    SetEnvironmentVariableW(L"TURRUN_PATH_PREPEND", NULL);

    SetHandleInformation(in, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
    SetHandleInformation(out, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
    SetHandleInformation(err, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);

    ZeroMemory(&si, sizeof(si));
    si.cb = sizeof(si);
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdInput = in;
    si.hStdOutput = out;
    si.hStdError = err;

    if (!CreateProcessW(NULL, cmdline, NULL, NULL, TRUE, 0, NULL, NULL, &si, &pi))
    {
        code = 251;
    }
    else
    {
        WaitForSingleObject(pi.hProcess, INFINITE);
        GetExitCodeProcess(pi.hProcess, &code);
        CloseHandle(pi.hProcess);
        CloseHandle(pi.hThread);
    }

    if (rcfile[0])
    {
        FILE *f = _wfopen(rcfile, L"w");
        if (f)
        {
            fprintf(f, "%lu\n", (unsigned long)code);
            fclose(f);
        }
    }
    return (int)code;
}
