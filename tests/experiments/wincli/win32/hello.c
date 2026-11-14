// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// hello.exe: a Windows console program for the integration tests (running
// an .exe by name from a Linux shell). It prints its arguments and working
// directory and exits with 7. Built in CI with mingw-w64; not shipped.

#include <windows.h>
#include <stdio.h>

int main(int argc, char **argv)
{
    char cwd[MAX_PATH] = "";
    int i;

    GetCurrentDirectoryA(MAX_PATH, cwd);
    printf("hello from exe\n");
    for (i = 1; i < argc; i++) printf("arg%d=%s\n", i, argv[i]);
    printf("cwd=%s\n", cwd);
    fflush(stdout);
    return 7;
}
