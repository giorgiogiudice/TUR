// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
//
// A Windows console program for the windows-runtime end-to-end test.
//
// It is compiled on the CI runner with mingw-w64 and run under Wine through
// `uos-runtime launch`. It prints what it could and could not do; the test
// driver (e2e.py) decides what that means. Nothing here is a judgement.
//
//   console.exe [EXIT-CODE [ARG...]]
//
// Environment, all optional, all Unix paths:
//   UOS_TEST_VALUE          printed back
//   UOS_TEST_HOST_CANARY    printed back; the launcher must not pass it on
//   UOS_TEST_GRANT_DIR      a directory to write a file into
//   UOS_TEST_READONLY_DIR   a directory to read from and try to write into
//   UOS_TEST_OUTSIDE_FILES  ';'-separated files to try to read
//
// Unix paths are reached through Z:, which Wine maps to the Unix root.

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define TAG "uos-windows-test: "

static void windows_path(char *out, size_t size, const char *unix_path, const char *leaf)
{
    snprintf(out, size, "Z:%s%s%s", unix_path, leaf ? "/" : "", leaf ? leaf : "");
    for (char *p = out; *p; ++p)
        if (*p == '/')
            *p = '\\';
}

/* Reads the first line of a file. Returns 0 when the file cannot be opened. */
static int first_line(const char *path, char *out, size_t size)
{
    FILE *file = fopen(path, "rb");
    if (!file)
        return 0;
    size_t length = fread(out, 1, size - 1, file);
    fclose(file);
    out[length] = '\0';
    out[strcspn(out, "\r\n")] = '\0';
    return 1;
}

static const char *printable(const char *value)
{
    return value ? value : "(unset)";
}

int main(int argc, char **argv)
{
    char path[2048];
    char text[128];
    int exit_code = argc > 1 ? atoi(argv[1]) : 0;

    setvbuf(stdout, NULL, _IONBF, 0);
    printf(TAG "hello from a Windows process\n");
    printf(TAG "pointer-bits=%u\n", (unsigned)(sizeof(void *) * 8));
    for (int i = 1; i < argc; ++i)
        printf(TAG "arg[%d]=%s\n", i, argv[i]);
    printf(TAG "env UOS_TEST_VALUE=%s\n", printable(getenv("UOS_TEST_VALUE")));
    printf(TAG "env UOS_TEST_HOST_CANARY=%s\n", printable(getenv("UOS_TEST_HOST_CANARY")));

    const char *grant = getenv("UOS_TEST_GRANT_DIR");
    if (grant) {
        windows_path(path, sizeof path, grant, "written-by-windows.txt");
        FILE *file = fopen(path, "wb");
        if (file && fputs("written by a Windows process\n", file) >= 0 && fclose(file) == 0)
            printf(TAG "grant-write: ok\n");
        else
            printf(TAG "grant-write: failed errno=%d\n", errno);
    }

    const char *readonly = getenv("UOS_TEST_READONLY_DIR");
    if (readonly) {
        windows_path(path, sizeof path, readonly, "readable.txt");
        if (first_line(path, text, sizeof text))
            printf(TAG "readonly-read: ok %s\n", text);
        else
            printf(TAG "readonly-read: failed errno=%d\n", errno);
        windows_path(path, sizeof path, readonly, "must-not-appear.txt");
        FILE *file = fopen(path, "wb");
        if (file) {
            fputs("a read-only grant was written to\n", file);
            fclose(file);
            printf(TAG "readonly-write: WRITTEN\n");
        } else {
            printf(TAG "readonly-write: denied errno=%d\n", errno);
        }
    }

    const char *outside = getenv("UOS_TEST_OUTSIDE_FILES");
    if (outside) {
        char *list = strdup(outside);
        for (char *item = strtok(list, ";"); item; item = strtok(NULL, ";")) {
            windows_path(path, sizeof path, item, NULL);
            if (first_line(path, text, sizeof text))
                printf(TAG "outside-read: LEAKED %s: %s\n", item, text);
            else
                printf(TAG "outside-read: denied %s\n", item);
        }
        free(list);
    }

    /* One line is added to a file inside the prefix at every run, so the
       driver can tell a reused prefix from a new one. */
    FILE *runs = fopen("C:\\uos-test-runs.txt", "ab");
    if (runs) {
        fputs("run\n", runs);
        fclose(runs);
    }
    int count = 0;
    runs = fopen("C:\\uos-test-runs.txt", "rb");
    if (runs) {
        for (int c = fgetc(runs); c != EOF; c = fgetc(runs))
            if (c == '\n')
                ++count;
        fclose(runs);
    }
    printf(TAG "prefix-runs=%d\n", count);

    printf(TAG "exiting with %d\n", exit_code);
    return exit_code;
}
