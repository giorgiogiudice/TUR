// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
//
// A native program, built several ways with gcc by the windows-runtime
// end-to-end test (position-independent, fixed-address, static, shared
// object, object file) so that `uos-runtime inspect` is checked against real
// ELF files rather than hand-written headers. It is inspected, never run.

#include <stdio.h>

int uos_test_answer(void)
{
    return 42;
}

int main(void)
{
    printf("uos-native-test: %d\n", uos_test_answer());
    return 0;
}
