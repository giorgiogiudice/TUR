// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// A Windows console program that looks at how file names are found on a
// prefix's drive C:: what a name in another case resolves to, and how long
// finding names takes. It is built on the CI runner with mingw-w64 and run
// under Wine through tur-runtime, once in a prefix whose drive_c is
// case-insensitive (ext4 case folding) and once in a prefix whose drive_c is
// an ordinary directory, on the same file system. casefold.py compares the
// two. Nothing here is a judgement: every result is printed as observed.
//
//   casefold.exe semantics DIR          name resolution, one test per line
//   casefold.exe bench DIR FILES OPS    timings of FILES files, OPS lookups
//   casefold.exe plugins                files beside the program, asked for
//                                       through a directory name in other case
//
// Every result is one line, "tur-casefold: " and a JSON object.

#define UNICODE
#define _UNICODE
#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0600
#endif
#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

#define TAG "tur-casefold: "
#define PATH_CHARS 1024

/* ------------------------------------------------------------ output */

static void put_wide(const wchar_t *text)
{
    putchar('"');
    for (const wchar_t *p = text; *p; ++p) {
        if (*p == L'"' || *p == L'\\')
            printf("\\%c", (char)*p);
        else if (*p >= 0x20 && *p < 0x7f)
            putchar((char)*p);
        else
            printf("\\u%04x", (unsigned)*p);
    }
    putchar('"');
}

static void begin(const char *kind, const char *name)
{
    printf(TAG "{\"%s\": \"%s\"", kind, name);
}

static void field_bool(const char *key, int value)
{
    printf(", \"%s\": %s", key, value ? "true" : "false");
}

static void field_int(const char *key, long value)
{
    printf(", \"%s\": %ld", key, value);
}

static void field_double(const char *key, double value)
{
    printf(", \"%s\": %.6f", key, value);
}

static void field_wide(const char *key, const wchar_t *value)
{
    printf(", \"%s\": ", key);
    put_wide(value);
}

static void end(void)
{
    printf("}\n");
}

/* ------------------------------------------------------------ files */

static void join(wchar_t *out, const wchar_t *dir, const wchar_t *name)
{
    _snwprintf(out, PATH_CHARS, L"%ls\\%ls", dir, name);
    out[PATH_CHARS - 1] = 0;
}

/* Creates or replaces a file. Returns 0 or the Windows error. */
static DWORD write_text(const wchar_t *path, const char *text, DWORD disposition)
{
    HANDLE file = CreateFileW(path, GENERIC_WRITE, 0, NULL, disposition, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE)
        return GetLastError();
    DWORD written = 0;
    WriteFile(file, text, (DWORD)strlen(text), &written, NULL);
    CloseHandle(file);
    return 0;
}

/* Reads up to size-1 bytes. Returns 0 or the Windows error. */
static DWORD read_text(const wchar_t *path, char *out, DWORD size)
{
    out[0] = 0;
    HANDLE file = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE)
        return GetLastError();
    DWORD got = 0;
    ReadFile(file, out, size - 1, &got, NULL);
    out[got] = 0;
    CloseHandle(file);
    return 0;
}

/* The names in DIR that match PATTERN, as a JSON array. */
static void field_listing(const char *key, const wchar_t *dir, const wchar_t *pattern)
{
    wchar_t mask[PATH_CHARS];
    WIN32_FIND_DATAW found;
    join(mask, dir, pattern);
    printf(", \"%s\": [", key);
    HANDLE search = FindFirstFileW(mask, &found);
    int first = 1;
    if (search != INVALID_HANDLE_VALUE) {
        do {
            if (!wcscmp(found.cFileName, L".") || !wcscmp(found.cFileName, L".."))
                continue;
            if (!first)
                printf(", ");
            put_wide(found.cFileName);
            first = 0;
        } while (FindNextFileW(search, &found));
        FindClose(search);
    }
    printf("]");
}

/* Opens ASKED after creating STORED, and says what was found. */
static void lookup_test(const char *name, const wchar_t *dir, const wchar_t *stored, const wchar_t *asked)
{
    wchar_t created[PATH_CHARS], wanted[PATH_CHARS];
    char content[64];
    join(created, dir, stored);
    join(wanted, dir, asked);
    DWORD made = write_text(created, name, CREATE_ALWAYS);
    DWORD error = read_text(wanted, content, sizeof content);
    begin("test", name);
    field_wide("stored", stored);
    field_wide("asked", asked);
    field_int("create_error", made);
    field_bool("found", error == 0);
    field_int("error", error);
    field_bool("same_file", error == 0 && !strcmp(content, name));
    end();
}

static int semantics(const wchar_t *dir)
{
    wchar_t path[PATH_CHARS], other[PATH_CHARS], result[PATH_CHARS];
    char content[64];
    DWORD error;

    CreateDirectoryW(dir, NULL);

    /* A file found under another case, and its content. */
    join(path, dir, L"Readme.Txt");
    write_text(path, "first", CREATE_ALWAYS);
    join(other, dir, L"README.TXT");
    error = read_text(other, content, sizeof content);
    begin("test", "open-other-case");
    field_bool("found", error == 0);
    field_int("error", error);
    field_bool("same_file", error == 0 && !strcmp(content, "first"));
    end();

    /* Creating a new file whose name differs only in case. */
    join(other, dir, L"readme.txt");
    error = write_text(other, "second", CREATE_NEW);
    begin("test", "create-new-other-case");
    field_bool("refused", error != 0);
    field_int("error", error);
    field_listing("names", dir, L"readme*");
    end();

    /* The name a search reports for a name asked for in another case. */
    {
        WIN32_FIND_DATAW found;
        join(other, dir, L"README.TXT");
        HANDLE search = FindFirstFileW(other, &found);
        begin("test", "find-first-file-name");
        field_bool("found", search != INVALID_HANDLE_VALUE);
        field_wide("name", search != INVALID_HANDLE_VALUE ? found.cFileName : L"");
        end();
        if (search != INVALID_HANDLE_VALUE)
            FindClose(search);
    }

    /* The long path name of a path asked for in another case. */
    {
        DWORD length = GetLongPathNameW(other, result, PATH_CHARS);
        begin("test", "long-path-name");
        field_bool("found", length > 0 && length < PATH_CHARS);
        field_wide("path", length > 0 && length < PATH_CHARS ? result : L"");
        end();
    }

    /* The final path of a file opened under another case. */
    {
        HANDLE file = CreateFileW(other, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
        DWORD length = 0;
        if (file != INVALID_HANDLE_VALUE) {
            length = GetFinalPathNameByHandleW(file, result, PATH_CHARS, FILE_NAME_NORMALIZED | VOLUME_NAME_DOS);
            CloseHandle(file);
        }
        begin("test", "final-path-name");
        field_bool("found", length > 0 && length < PATH_CHARS);
        field_wide("path", length > 0 && length < PATH_CHARS ? result : L"");
        end();
    }

    /* Renaming a file to the same name in other case. */
    {
        join(path, dir, L"Readme.Txt");
        join(other, dir, L"README.TXT");
        BOOL moved = MoveFileW(path, other);
        error = moved ? 0 : GetLastError();
        begin("test", "case-only-rename");
        field_bool("moved", moved);
        field_int("error", error);
        field_listing("names", dir, L"readme*");
        end();
    }

    /* A directory, then a file through the directory's name in other case. */
    {
        join(path, dir, L"Sub");
        BOOL made = CreateDirectoryW(path, NULL);
        join(other, dir, L"SUB\\inner.txt");
        error = write_text(other, "inner", CREATE_ALWAYS);
        begin("test", "directory-other-case");
        field_bool("directory_created", made);
        field_bool("file_created", error == 0);
        field_int("error", error);
        field_listing("names", dir, L"sub");
        end();
    }

    /* Letters outside ASCII. */
    lookup_test("latin-letter-other-case", dir, L"\x00c4rger.txt", L"\x00e4RGER.TXT");
    lookup_test("greek-letter-other-case", dir, L"\x039f\x0394\x039f\x03a3.txt", L"\x03bf\x03b4\x03bf\x03c3.txt");
    lookup_test("cyrillic-letter-other-case", dir, L"\x0424\x0430\x0439\x043b.txt", L"\x0444\x0410\x0419\x041b.TXT");
    /* Two spellings of one letter: precomposed and decomposed. */
    lookup_test("composed-and-decomposed", dir, L"caf\x00e9.txt", L"cafe\x0301.txt");
    /* A letter whose case folding is two letters. */
    lookup_test("sharp-s-and-ss", dir, L"stra\x00df" L"e.txt", L"STRASSE.TXT");
    /* A sign that folds to a letter. The literal is split: "e" would
       otherwise be read as a further hexadecimal digit of the escape. */
    lookup_test("kelvin-sign-and-k", dir, L"key.txt", L"\x212a" L"ey.txt");
    /* Dotted and dotless i. */
    lookup_test("dotless-i-and-i", dir, L"fix.txt", L"f\x0131x.txt");

    /* After the above: may a file whose name only differs by composition be created? */
    join(other, dir, L"cafe\x0301-new.txt");
    join(path, dir, L"caf\x00e9-new.txt");
    write_text(path, "composed", CREATE_ALWAYS);
    error = write_text(other, "decomposed", CREATE_NEW);
    begin("test", "create-decomposed-beside-composed");
    field_bool("created", error == 0);
    field_int("error", error);
    field_listing("names", dir, L"caf*-new.txt");
    end();
    return 0;
}

/* ------------------------------------------------------------ bench */

static double now_seconds(void)
{
    static LARGE_INTEGER frequency;
    LARGE_INTEGER counter;
    if (!frequency.QuadPart)
        QueryPerformanceFrequency(&frequency);
    QueryPerformanceCounter(&counter);
    return (double)counter.QuadPart / (double)frequency.QuadPart;
}

enum operation { OPEN, FIND, ATTRIBUTES, CREATE };

static int attempt(enum operation operation, const wchar_t *path)
{
    if (operation == FIND) {
        WIN32_FIND_DATAW found;
        HANDLE search = FindFirstFileW(path, &found);
        if (search == INVALID_HANDLE_VALUE)
            return 0;
        FindClose(search);
        return 1;
    }
    if (operation == ATTRIBUTES)
        return GetFileAttributesW(path) != INVALID_FILE_ATTRIBUTES;
    HANDLE file = CreateFileW(path, operation == CREATE ? GENERIC_WRITE : GENERIC_READ, FILE_SHARE_READ, NULL,
                              operation == CREATE ? CREATE_NEW : OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE)
        return 0;
    CloseHandle(file);
    return 1;
}

/* Times COUNT operations on names made from FORMAT and an index. */
static void measure(const char *name, enum operation operation, const wchar_t *dir, const wchar_t *format,
                    int files, int count, int sequential)
{
    wchar_t leaf[64], path[PATH_CHARS];
    int succeeded = 0;
    double started = now_seconds();
    for (int i = 0; i < count; ++i) {
        int index = sequential ? i : (int)(((long long)i * 7919) % files);
        _snwprintf(leaf, 64, format, index);
        leaf[63] = 0;
        join(path, dir, leaf);
        succeeded += attempt(operation, path);
    }
    double seconds = now_seconds() - started;
    begin("bench", name);
    field_int("files", files);
    field_int("operations", count);
    field_int("succeeded", succeeded);
    field_double("seconds", seconds);
    field_double("microseconds_per_operation", count ? seconds * 1e6 / count : 0);
    end();
}

static int bench(const wchar_t *base, int files, int count)
{
    wchar_t dir[PATH_CHARS];
    CreateDirectoryW(base, NULL);
    _snwprintf(dir, PATH_CHARS, L"%ls\\n%d", base, files);
    dir[PATH_CHARS - 1] = 0;
    CreateDirectoryW(dir, NULL);

    /* Making the files is measured too: every new name is first looked for. */
    measure("create-long-names", CREATE, dir, L"longname-%05d.data", files, files, 1);
    measure("create-short-names", CREATE, dir, L"f%05d.dat", files, files, 1);

    measure("open-same-case-long", OPEN, dir, L"longname-%05d.data", files, count, 0);
    measure("open-other-case-long", OPEN, dir, L"LONGNAME-%05d.DATA", files, count, 0);
    measure("open-other-case-short", OPEN, dir, L"F%05d.DAT", files, count, 0);
    measure("open-missing-long", OPEN, dir, L"LONGNAME-MISSING-%05d.DATA", files, count, 0);
    measure("open-missing-short", OPEN, dir, L"M%05d.DAT", files, count, 0);
    measure("attributes-other-case-long", ATTRIBUTES, dir, L"LONGNAME-%05d.DATA", files, count, 0);
    measure("find-same-case-long", FIND, dir, L"longname-%05d.data", files, count, 0);
    measure("find-other-case-long", FIND, dir, L"LONGNAME-%05d.DATA", files, count, 0);
    return 0;
}

/* ------------------------------------------------------------ plugins */

/* The program's own directory holds Plugins\one.txt and plugins\two.txt, as
   unpacked from an archive that spelt the directory two ways. */
static int plugins(void)
{
    wchar_t self[PATH_CHARS], path[PATH_CHARS];
    char content[64];
    GetModuleFileNameW(NULL, self, PATH_CHARS);
    wchar_t *slash = wcsrchr(self, L'\\');
    if (slash)
        *slash = 0;
    static const wchar_t *asked[] = {
        L"Plugins\\one.txt", L"plugins\\two.txt", L"PLUGINS\\one.txt", L"PLUGINS\\two.txt",
        L"Plugins\\two.txt", L"plugins\\one.txt",
    };
    for (size_t i = 0; i < sizeof asked / sizeof asked[0]; ++i) {
        join(path, self, asked[i]);
        DWORD error = read_text(path, content, sizeof content);
        begin("plugins", "open");
        field_wide("asked", asked[i]);
        field_bool("found", error == 0);
        field_int("error", error);
        end();
    }
    begin("plugins", "listing");
    field_listing("directories", self, L"plugins");
    end();
    return 0;
}

int main(int argc, char **argv)
{
    wchar_t dir[PATH_CHARS];
    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc >= 2 && !strcmp(argv[1], "plugins"))
        return plugins();
    if (argc < 3) {
        fprintf(stderr, "usage: casefold.exe semantics DIR | bench DIR FILES OPS | plugins\n");
        return 2;
    }
    MultiByteToWideChar(CP_ACP, 0, argv[2], -1, dir, PATH_CHARS);
    if (!strcmp(argv[1], "semantics"))
        return semantics(dir);
    if (!strcmp(argv[1], "bench") && argc >= 5)
        return bench(dir, atoi(argv[3]), atoi(argv[4]));
    fprintf(stderr, "unknown mode %s\n", argv[1]);
    return 2;
}
