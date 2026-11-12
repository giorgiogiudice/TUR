// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// Stand-ins for installers that go wrong, for the end-to-end test of
// `tur-runtime app install`. Real installers are tried by apps.py; these
// exist because a real installer cannot be asked to fail on demand.
//
// build.sh compiles this three times and appends the NSIS first header to
// each result, so that tur-runtime identifies the file as an NSIS installer
// and runs it with /S, as it would a real one:
//
//   installer-fails.exe     exits with status 3
//   installer-hangs.exe     never finishes
//   installer-nothing.exe   exits with status 0 and installs nothing
//
// Nothing here installs anything.

#include <stdio.h>
#include <windows.h>

int main(void)
{
#if defined(TUR_INSTALLER_FAILS)
    fputs("tur-test-installer: failing on purpose\n", stderr);
    return 3;
#elif defined(TUR_INSTALLER_HANGS)
    fputs("tur-test-installer: waiting for an answer that never comes\n", stderr);
    fflush(stderr);
    Sleep(60 * 60 * 1000);
    return 0;
#else
    fputs("tur-test-installer: done, and nothing was installed\n", stderr);
    return 0;
#endif
}
