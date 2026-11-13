/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * A deliberately broken program for the compatibility autopilot's CI: it
 * was "written for a Japanese Windows" (its resources, codepage.rc, are in
 * Japanese) and crashes on any ANSI code page but 932, as programs that
 * index tables by Shift-JIS lead bytes do. Wine takes the code page from
 * the Unix locale (dlls/ntdll/unix/env.c, init_locale), so the environment
 * setting LC_ALL=ja_JP.UTF-8 is what it needs.
 */
#include <windows.h>
#include <stdio.h>

int main(void)
{
    UINT code_page = GetACP();

    printf("ANSI code page %u\n", code_page);
    fflush(stdout);
    if (code_page != 932) {
        /* The lead-byte table this program would use does not exist: the
         * access violation such a program would cause, raised explicitly
         * (a store to a low address is not reliably a fault under Wine). */
        RaiseException(EXCEPTION_ACCESS_VIOLATION, EXCEPTION_NONCONTINUABLE, 0, NULL);
        return 6;
    }
    printf("TUR-CHECK-PASS codepage\n");
    return 0;
}
