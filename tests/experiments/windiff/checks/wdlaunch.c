/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * windiff: starts a check program the same way on Windows and under Wine.
 *
 *     wdlaunch.exe PROGRAM ARGUMENTS...
 *
 * The program gets a console without a window (CREATE_NO_WINDOW), this
 * process's standard handles, and a job that ends it and everything it
 * started when this process ends, so that a runner that kills the launcher
 * after a timeout leaves nothing behind. Without this the starting point
 * differs: a program started from a script has a console on Windows and
 * none under Wine.
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>

int wmain(void)
{
    WCHAR *cl = GetCommandLineW();
    STARTUPINFOW si;
    PROCESS_INFORMATION pi;
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits;
    HANDLE job;
    DWORD code = 0xfd;

    if (*cl == '"') { cl++; while (*cl && *cl != '"') cl++; if (*cl) cl++; }
    else while (*cl && *cl != ' ' && *cl != '\t') cl++;
    while (*cl == ' ' || *cl == '\t') cl++;
    if (!*cl) return 0xfc;

    SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
    job = CreateJobObjectW(NULL, NULL);
    memset(&limits, 0, sizeof(limits));
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if (job) SetInformationJobObject(job, JobObjectExtendedLimitInformation, &limits, sizeof(limits));
    memset(&si, 0, sizeof(si));
    si.cb = sizeof(si);
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    si.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
    si.hStdError = GetStdHandle(STD_ERROR_HANDLE);
    if (!CreateProcessW(NULL, cl, NULL, NULL, TRUE, CREATE_NO_WINDOW | CREATE_SUSPENDED, NULL, NULL, &si, &pi)) return 0xfd;
    if (job) AssignProcessToJobObject(job, pi.hProcess);
    ResumeThread(pi.hThread);
    WaitForSingleObject(pi.hProcess, INFINITE);
    GetExitCodeProcess(pi.hProcess, &code);
    return (int)code;
}
