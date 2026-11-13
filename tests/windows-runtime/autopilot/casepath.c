/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * A deliberately broken program for the compatibility autopilot's CI: its
 * archive spells one directory two ways, Plugins/one.txt and
 * plugins/two.txt, as archives put together on case-sensitive systems
 * sometimes do. It opens both as Plugins\. On Windows, and on a
 * case-insensitive drive C:, that is one directory; on an ordinary Linux
 * directory Wine finds "Plugins" and two.txt is not in it
 * (docs/reports/windows-runtime-3.md).
 */
#include <windows.h>
#include <stdio.h>
#include <string.h>

static int read_one(const char *directory, const char *name, const char *expected)
{
    char path[MAX_PATH * 2];
    char text[64];
    FILE *file;
    size_t read;

    snprintf(path, sizeof(path), "%s\\Plugins\\%s", directory, name);
    file = fopen(path, "rb");
    if (file == NULL) {
        fprintf(stderr, "cannot open Plugins\\%s: no such file\n", name);
        return 0;
    }
    read = fread(text, 1, sizeof(text) - 1, file);
    fclose(file);
    text[read] = '\0';
    if (strncmp(text, expected, strlen(expected)) != 0) {
        fprintf(stderr, "Plugins\\%s does not hold what it should\n", name);
        return 0;
    }
    printf("read Plugins\\%s\n", name);
    return 1;
}

int main(void)
{
    char directory[MAX_PATH];
    char *slash;
    int ok;

    if (GetModuleFileNameA(NULL, directory, MAX_PATH) == 0)
        return 2;
    slash = strrchr(directory, '\\');
    if (slash != NULL)
        *slash = '\0';
    ok = read_one(directory, "one.txt", "one");
    ok = read_one(directory, "two.txt", "two") && ok;
    fflush(stdout);
    if (!ok)
        return 5;
    printf("TUR-CHECK-PASS casepath\n");
    return 0;
}
