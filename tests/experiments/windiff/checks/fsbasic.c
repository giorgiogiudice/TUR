/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * windiff, file-system semantics 1: sharing modes, deletion, creation
 * dispositions, attributes, times, links, renaming, copying, reading and
 * writing, locks, directory enumeration.
 */
#include "wd.h"

static FILETIME ft_of(int year, int month, int day, int hour)
{
    SYSTEMTIME st;
    FILETIME ft = {0, 0};
    memset(&st, 0, sizeof(st));
    st.wYear = (WORD)year; st.wMonth = (WORD)month; st.wDay = (WORD)day; st.wHour = (WORD)hour;
    SystemTimeToFileTime(&st, &ft);
    return ft;
}

static const char *ft_str(const FILETIME *ft)
{
    static char b[4][48];
    static int i;
    char *p = b[i++ & 3];
    SYSTEMTIME st;
    if (!FileTimeToSystemTime(ft, &st)) _snprintf(p, 48, "raw=%08lx:%08lx", ft->dwHighDateTime, ft->dwLowDateTime);
    else _snprintf(p, 48, "%04u-%02u-%02uT%02u:%02u:%02u.%03u", st.wYear, st.wMonth, st.wDay, st.wHour,
                   st.wMinute, st.wSecond, st.wMilliseconds);
    return p;
}

/* ---- sharing modes ---------------------------------------------------- */

static void g_share(void)
{
    static const struct { const char *n; DWORD acc, shr; } m[] = {
        {"r-0", GENERIC_READ, 0}, {"r-r", GENERIC_READ, FILE_SHARE_READ}, {"r-w", GENERIC_READ, FILE_SHARE_WRITE},
        {"r-rw", GENERIC_READ, WD_RW}, {"r-rwd", GENERIC_READ, WD_RWD}, {"r-d", GENERIC_READ, FILE_SHARE_DELETE},
        {"w-0", GENERIC_WRITE, 0}, {"w-r", GENERIC_WRITE, FILE_SHARE_READ}, {"w-w", GENERIC_WRITE, FILE_SHARE_WRITE},
        {"w-rw", GENERIC_WRITE, WD_RW}, {"w-rwd", GENERIC_WRITE, WD_RWD},
        {"rw-0", GENERIC_READ | GENERIC_WRITE, 0}, {"rw-rw", GENERIC_READ | GENERIC_WRITE, WD_RW},
        {"rw-rwd", GENERIC_READ | GENERIC_WRITE, WD_RWD},
        {"d-0", DELETE, 0}, {"d-rw", DELETE, WD_RW}, {"d-rwd", DELETE, WD_RWD}, {"d-d", DELETE, FILE_SHARE_DELETE},
        {"none-0", 0, 0}, {"ra-0", FILE_READ_ATTRIBUTES, 0}, {"wa-0", FILE_WRITE_ATTRIBUTES, 0},
        {"x-r", GENERIC_EXECUTE, FILE_SHARE_READ}, {"ap-rw", FILE_APPEND_DATA, WD_RW}, {"sy-0", SYNCHRONIZE, 0},
    };
    char row[1500];
    unsigned i, j;
    int n;
    HANDLE f, map, h2;
    void *view;

    wd_mkfile(L"s.dat", "data");
    for (i = 0; i < ARRAY_SIZE(m); i++)
    {
        HANDLE h1 = wd_open(L"s.dat", m[i].acc, m[i].shr, OPEN_EXISTING, 0);
        char id[40];
        _snprintf(id, sizeof(id), "first.%s", m[i].n);
        if (h1 == INVALID_HANDLE_VALUE) { wd_emit(id, "first open: e=%lu", GetLastError()); continue; }
        n = 0;
        for (j = 0; j < ARRAY_SIZE(m); j++)
        {
            h2 = wd_open(L"s.dat", m[j].acc, m[j].shr, OPEN_EXISTING, 0);
            if (h2 != INVALID_HANDLE_VALUE) { n += _snprintf(row + n, 40, "%s=ok ", m[j].n); CloseHandle(h2); }
            else n += _snprintf(row + n, 40, "%s=%lu ", m[j].n, GetLastError());
        }
        row[n] = 0;
        CloseHandle(h1);
        wd_emit(id, "%s", row);
    }

    /* a file that is mapped */
    wd_mkfile(L"m.dat", "0123456789");
    f = wd_open(L"m.dat", GENERIC_READ | GENERIC_WRITE, WD_RWD, OPEN_EXISTING, 0);
    map = CreateFileMappingW(f, NULL, PAGE_READWRITE, 0, 0, NULL);
    EMIT_B("map.create", map != NULL);
    view = map ? MapViewOfFile(map, FILE_MAP_WRITE, 0, 0, 0) : NULL;
    EMIT_B("map.view", view != NULL);
    SetFilePointer(f, 4, NULL, FILE_BEGIN);
    EMIT_B("map.truncate-with-view", SetEndOfFile(f));
    SetFilePointer(f, 100, NULL, FILE_BEGIN);
    EMIT_B("map.extend-with-view", SetEndOfFile(f));
    h2 = wd_open(L"m.dat", GENERIC_WRITE, WD_RWD, TRUNCATE_EXISTING, 0);
    EMIT_H("map.open-truncate-existing-with-view", h2); CLOSE(h2);
    h2 = wd_open(L"m.dat", GENERIC_WRITE, WD_RWD, CREATE_ALWAYS, 0);
    EMIT_H("map.open-create-always-with-view", h2); CLOSE(h2);
    if (view) UnmapViewOfFile(view);
    SetFilePointer(f, 4, NULL, FILE_BEGIN);
    EMIT_B("map.truncate-with-section-only", SetEndOfFile(f));
    if (map) CloseHandle(map);
    SetFilePointer(f, 4, NULL, FILE_BEGIN);
    EMIT_B("map.truncate-after-section-closed", SetEndOfFile(f));
    CLOSE(f);

    wd_mkfile(L"m2.dat", "0123456789");
    f = wd_open(L"m2.dat", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    map = CreateFileMappingW(f, NULL, PAGE_READONLY, 0, 0, NULL);
    view = map ? MapViewOfFile(map, FILE_MAP_READ, 0, 0, 0) : NULL;
    CLOSE(f);
    EMIT_B("map.delete-with-view-no-handle", DeleteFileW(L"m2.dat"));
    wd_emit("map.attr-after-delete-with-view", "%s", wd_attr(L"m2.dat"));
    if (view) UnmapViewOfFile(view);
    if (map) CloseHandle(map);
    wd_emit("map.attr-after-unmap", "%s", wd_attr(L"m2.dat"));

    /* a zero-length file cannot be mapped */
    wd_mkfile(L"z.dat", "");
    f = wd_open(L"z.dat", GENERIC_READ | GENERIC_WRITE, WD_RWD, OPEN_EXISTING, 0);
    map = CreateFileMappingW(f, NULL, PAGE_READONLY, 0, 0, NULL);
    EMIT_B("map.zero-length-file", map != NULL);
    if (map) CloseHandle(map);
    map = CreateFileMappingW(f, NULL, PAGE_READWRITE, 0, 4096, NULL);
    EMIT_B("map.zero-length-file-grow", map != NULL);
    wd_emit("map.size-after-grow", "%lu", GetFileSize(f, NULL));
    if (map) CloseHandle(map);
    CLOSE(f);
}

/* ---- deletion --------------------------------------------------------- */

static void g_delete(void)
{
    HANDLE h, h2, h3;
    struct { BOOLEAN del; } di; /* FILE_DISPOSITION_INFO */
    struct { ULONG Flags; } dix; /* FILE_DISPOSITION_INFO_EX */
    FILE_STANDARD_INFO si;
    BY_HANDLE_FILE_INFORMATION bi;
    WCHAR full[700];
    char buf[8];
    DWORD n;
    BOOL ok;

    h = wd_open(L"d1", GENERIC_WRITE, 0, CREATE_NEW, FILE_FLAG_DELETE_ON_CLOSE);
    EMIT_H("doc.create", h);
    wd_emit("doc.attr-while-open", "%s", wd_attr(L"d1"));
    CLOSE(h);
    wd_emit("doc.attr-after-close", "%s", wd_attr(L"d1"));

    wd_mkfile(L"d2", "x");
    h = wd_open(L"d2", GENERIC_READ, WD_RW, OPEN_EXISTING, FILE_FLAG_DELETE_ON_CLOSE);
    EMIT_H("doc.open-existing-share-rw", h);
    h2 = wd_open(L"d2", GENERIC_READ, WD_RW, OPEN_EXISTING, 0);
    EMIT_H("doc.second-open-share-rw", h2); CLOSE(h2);
    h2 = wd_open(L"d2", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("doc.second-open-share-rwd", h2);
    CLOSE(h);
    wd_emit("doc.attr-after-first-close", "%s", wd_attr(L"d2"));
    h3 = wd_open(L"d2", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("doc.open-while-delete-pending", h3); CLOSE(h3);
    h3 = wd_open(L"d2", GENERIC_WRITE, WD_RWD, CREATE_ALWAYS, 0);
    EMIT_H("doc.create-always-while-delete-pending", h3); CLOSE(h3);
    CLOSE(h2);
    wd_emit("doc.attr-after-all-closed", "%s", wd_attr(L"d2"));
    wd_clean();

    wd_mkfile(L"d3", "x");
    h = wd_open(L"d3", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    h2 = wd_open(L"d3", GENERIC_READ, WD_RWD, OPEN_EXISTING, FILE_FLAG_DELETE_ON_CLOSE);
    EMIT_H("doc.second-handle-is-doc", h2); CLOSE(h2);
    wd_emit("doc.attr-after-doc-handle-closed", "%s", wd_attr(L"d3"));
    ok = wd_ok(ReadFile(h, buf, 1, &n, NULL));
    wd_emit("doc.read-through-first-handle", ok ? "ok n=%lu" : "e=%lu", ok ? n : wd_last);
    CLOSE(h);
    wd_emit("doc.attr-at-end", "%s", wd_attr(L"d3"));
    wd_clean();

    /* read-only files */
    wd_mkfile(L"ro", "x");
    SetFileAttributesW(L"ro", FILE_ATTRIBUTE_READONLY);
    EMIT_B("readonly.DeleteFile", DeleteFileW(L"ro"));
    h = wd_open(L"ro", GENERIC_READ, WD_RWD, OPEN_EXISTING, FILE_FLAG_DELETE_ON_CLOSE);
    EMIT_H("readonly.delete-on-close", h); CLOSE(h);
    wd_emit("readonly.attr-after-doc", "%s", wd_attr(L"ro"));
    h = wd_open(L"ro", DELETE, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("readonly.open-DELETE", h);
    di.del = TRUE;
    EMIT_B("readonly.FileDispositionInfo", SetFileInformationByHandle(h, FileDispositionInfo, &di, sizeof(di)));
    dix.Flags = 0x1 | 0x2 | 0x10; /* DELETE, POSIX_SEMANTICS, IGNORE_READONLY_ATTRIBUTE */
    EMIT_B("readonly.FileDispositionInfoEx-ignore-readonly",
           SetFileInformationByHandle(h, (FILE_INFO_BY_HANDLE_CLASS)21, &dix, sizeof(dix)));
    CLOSE(h);
    wd_emit("readonly.attr-after-ex", "%s", wd_attr(L"ro"));
    wd_clean();

    /* DeleteFile on a file that is open */
    wd_mkfile(L"o1", "x");
    h = wd_open(L"o1", GENERIC_READ, WD_RW, OPEN_EXISTING, 0);
    EMIT_B("open.share-rw.DeleteFile", DeleteFileW(L"o1"));
    CLOSE(h);
    wd_clean();
    wd_mkfile(L"o2", "x");
    h = wd_open(L"o2", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_B("open.share-rwd.DeleteFile", DeleteFileW(L"o2"));
    wd_emit("open.share-rwd.attr-after-delete", "%s", wd_attr(L"o2"));
    wd_emit("open.share-rwd.listing-after-delete", "%s", wd_ls(L"."));
    h2 = wd_open(L"o2", GENERIC_WRITE, WD_RWD, CREATE_NEW, 0);
    EMIT_H("open.share-rwd.create-same-name", h2); CLOSE(h2);
    ok = wd_ok(GetFileInformationByHandle(h, &bi));
    wd_emit("open.share-rwd.links-after-delete", ok ? "links=%lu" : "e=%lu", ok ? bi.nNumberOfLinks : wd_last);
    ok = wd_ok(ReadFile(h, buf, 1, &n, NULL));
    wd_emit("open.share-rwd.read-after-delete", ok ? "ok n=%lu" : "e=%lu", ok ? n : wd_last);
    EMIT_B("open.share-rwd.DeleteFile-again", DeleteFileW(L"o2"));
    CLOSE(h);
    wd_emit("open.share-rwd.attr-after-close", "%s", wd_attr(L"o2"));
    wd_clean();

    /* names that cannot be deleted */
    CreateDirectoryW(L"dir", NULL);
    wd_mkfile(L"dir\\f", "x");
    EMIT_B("dir.DeleteFile-on-directory", DeleteFileW(L"dir"));
    EMIT_B("dir.RemoveDirectory-not-empty", RemoveDirectoryW(L"dir"));
    EMIT_B("dir.RemoveDirectory-on-file", RemoveDirectoryW(L"dir\\f"));
    EMIT_B("dir.RemoveDirectory-missing", RemoveDirectoryW(L"nodir"));
    EMIT_B("dir.RemoveDirectory-missing-parent", RemoveDirectoryW(L"nodir\\sub"));
    EMIT_B("dir.RemoveDirectory-empty-name", RemoveDirectoryW(L""));
    EMIT_B("file.DeleteFile-missing", DeleteFileW(L"nofile"));
    EMIT_B("file.DeleteFile-missing-parent", DeleteFileW(L"nodir\\nofile"));
    EMIT_B("file.DeleteFile-empty-name", DeleteFileW(L""));
    EMIT_B("file.DeleteFile-trailing-backslash", DeleteFileW(L"dir\\f\\"));
    EMIT_B("file.DeleteFile-below-a-file", DeleteFileW(L"dir\\f\\x"));
    EMIT_B("file.DeleteFile-wildcard", DeleteFileW(L"dir\\*"));
    EMIT_B("file.DeleteFile-dot", DeleteFileW(L"."));
    h = wd_open(L"dir", GENERIC_READ, WD_RWD, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_DELETE_ON_CLOSE);
    EMIT_H("dir.delete-on-close-not-empty", h); CLOSE(h);
    wd_emit("dir.attr-after-doc-not-empty", "%s", wd_attr(L"dir"));
    DeleteFileW(L"dir\\f");
    h = wd_open(L"dir", GENERIC_READ, WD_RWD, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_DELETE_ON_CLOSE);
    EMIT_H("dir.delete-on-close-empty", h); CLOSE(h);
    wd_emit("dir.attr-after-doc-empty", "%s", wd_attr(L"dir"));
    wd_clean();

    CreateDirectoryW(L"cur", NULL);
    _snwprintf(full, 690, L"%ls\\cur", wd_cwd);
    SetCurrentDirectoryW(full);
    EMIT_B("dir.RemoveDirectory-current-directory", RemoveDirectoryW(full));
    SetCurrentDirectoryW(wd_cwd);
    wd_emit("dir.attr-after-remove-current", "%s", wd_attr(L"cur"));
    wd_clean();

    CreateDirectoryW(L"od", NULL);
    h = wd_open(L"od", GENERIC_READ, WD_RW, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS);
    EMIT_B("dir.RemoveDirectory-open-share-rw", RemoveDirectoryW(L"od"));
    CLOSE(h);
    h = wd_open(L"od", GENERIC_READ, WD_RWD, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS);
    EMIT_B("dir.RemoveDirectory-open-share-rwd", RemoveDirectoryW(L"od"));
    wd_emit("dir.attr-after-remove-open", "%s", wd_attr(L"od"));
    EMIT_B("dir.CreateDirectory-same-name-while-open", CreateDirectoryW(L"od", NULL));
    CLOSE(h);
    wd_clean();

    /* deletion through a handle */
    wd_mkfile(L"h1", "x");
    h = wd_open(L"h1", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    di.del = TRUE;
    EMIT_B("disp.without-DELETE-access", SetFileInformationByHandle(h, FileDispositionInfo, &di, sizeof(di)));
    CLOSE(h);
    h = wd_open(L"h1", DELETE, WD_RWD, OPEN_EXISTING, 0);
    EMIT_B("disp.set", SetFileInformationByHandle(h, FileDispositionInfo, &di, sizeof(di)));
    wd_emit("disp.attr-while-pending", "%s", wd_attr(L"h1"));
    h2 = wd_open(L"h1", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("disp.open-while-pending", h2); CLOSE(h2);
    ok = wd_ok(GetFileInformationByHandleEx(h, FileStandardInfo, &si, sizeof(si)));
    if (ok) wd_emit("disp.FileStandardInfo-while-pending", "pending=%d links=%lu", si.DeletePending, si.NumberOfLinks);
    else wd_emit("disp.FileStandardInfo-while-pending", "e=%lu", GetLastError());
    di.del = FALSE;
    EMIT_B("disp.undo", SetFileInformationByHandle(h, FileDispositionInfo, &di, sizeof(di)));
    CLOSE(h);
    wd_emit("disp.attr-after-undo", "%s", wd_attr(L"h1"));
    h = wd_open(L"h1", DELETE, WD_RWD, OPEN_EXISTING, 0);
    dix.Flags = 0x1 | 0x2;
    EMIT_B("disp.ex-posix", SetFileInformationByHandle(h, (FILE_INFO_BY_HANDLE_CLASS)21, &dix, sizeof(dix)));
    wd_emit("disp.ex-posix-attr-while-open", "%s", wd_attr(L"h1"));
    CLOSE(h);
    wd_mkfile(L"h2", "x");
    h = wd_open(L"h2", DELETE, WD_RWD, OPEN_EXISTING, 0);
    dix.Flags = 0x1;
    EMIT_B("disp.ex-not-posix", SetFileInformationByHandle(h, (FILE_INFO_BY_HANDLE_CLASS)21, &dix, sizeof(dix)));
    wd_emit("disp.ex-not-posix-attr-while-open", "%s", wd_attr(L"h2"));
    dix.Flags = 0;
    EMIT_B("disp.ex-undo", SetFileInformationByHandle(h, (FILE_INFO_BY_HANDLE_CLASS)21, &dix, sizeof(dix)));
    CLOSE(h);
    wd_emit("disp.ex-attr-after-undo", "%s", wd_attr(L"h2"));
}

/* ---- creation dispositions and attributes on create ------------------- */

static void g_create(void)
{
    static const struct { const char *n; DWORD d; } disp[] = {
        {"CREATE_NEW", 1}, {"CREATE_ALWAYS", 2}, {"OPEN_EXISTING", 3}, {"OPEN_ALWAYS", 4},
        {"TRUNCATE_EXISTING", 5}, {"disposition-0", 0}, {"disposition-6", 6},
    };
    static const struct { const char *n; int dir; DWORD attr; } state[] = {
        {"missing", -1, 0}, {"file", 0, FILE_ATTRIBUTE_NORMAL}, {"directory", 1, 0},
        {"readonly", 0, FILE_ATTRIBUTE_READONLY}, {"hidden", 0, FILE_ATTRIBUTE_HIDDEN},
        {"system", 0, FILE_ATTRIBUTE_SYSTEM},
    };
    unsigned i, j;
    HANDLE h;
    char id[80], buf[8];
    DWORD n, gle;
    BOOL ok;

    for (i = 0; i < ARRAY_SIZE(state); i++)
        for (j = 0; j < ARRAY_SIZE(disp); j++)
        {
            wd_clean();
            if (state[i].dir == 1) CreateDirectoryW(L"t", NULL);
            else if (state[i].dir == 0) { wd_mkfile(L"t", "12345"); SetFileAttributesW(L"t", state[i].attr); }
            _snprintf(id, sizeof(id), "disp.%s.%s", state[i].n, disp[j].n);
            SetLastError(0xdeadbeef);
            h = CreateFileW(L"t", GENERIC_READ | GENERIC_WRITE, 0, NULL, disp[j].d, FILE_ATTRIBUTE_NORMAL, NULL);
            gle = GetLastError();
            if (h == INVALID_HANDLE_VALUE) wd_emit(id, "e=%lu", gle);
            else
            {
                DWORD size = GetFileSize(h, NULL);
                CloseHandle(h);
                if (disp[j].d == 2 || disp[j].d == 4) wd_emit(id, "ok gle=%lu size=%lu %s", gle, size, wd_attr(L"t"));
                else wd_emit(id, "ok size=%lu %s", size, wd_attr(L"t"));
            }
        }
    wd_clean();

    wd_mkfile(L"t", "12345");
    h = wd_open(L"t", GENERIC_READ, 0, TRUNCATE_EXISTING, 0);
    EMIT_H("truncate-existing-read-only-access", h); CLOSE(h);
    SetFileAttributesW(L"t", FILE_ATTRIBUTE_HIDDEN);
    h = wd_open(L"t", GENERIC_WRITE, 0, CREATE_ALWAYS, FILE_ATTRIBUTE_HIDDEN);
    EMIT_H("create-always-hidden-over-hidden", h); CLOSE(h);
    h = wd_open(L"t", GENERIC_WRITE, 0, CREATE_ALWAYS, FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM);
    EMIT_H("create-always-hidden-system-over-hidden", h); CLOSE(h);
    wd_emit("attr-after-create-always-hidden-system", "%s", wd_attr(L"t"));
    h = wd_open(L"t", GENERIC_WRITE, 0, CREATE_ALWAYS, FILE_ATTRIBUTE_HIDDEN);
    EMIT_H("create-always-hidden-over-hidden-system", h); CLOSE(h);
    h = wd_open(L"t", GENERIC_WRITE, 0, TRUNCATE_EXISTING, 0);
    EMIT_H("truncate-existing-over-hidden-system", h); CLOSE(h);
    h = wd_open(L"t", GENERIC_WRITE, 0, OPEN_ALWAYS, 0);
    EMIT_H("open-always-over-hidden-system", h); CLOSE(h);
    wd_clean();

    h = wd_open(L"n1", GENERIC_WRITE, 0, CREATE_NEW, FILE_ATTRIBUTE_READONLY);
    EMIT_H("create-new-readonly", h); CLOSE(h);
    wd_emit("attr-of-new-readonly", "%s", wd_attr(L"n1"));
    h = wd_open(L"n2", GENERIC_WRITE, 0, CREATE_NEW, FILE_ATTRIBUTE_TEMPORARY);
    CLOSE(h);
    wd_emit("attr-of-new-temporary", "%s", wd_attr(L"n2"));
    h = wd_open(L"n3", GENERIC_WRITE, 0, CREATE_NEW, FILE_ATTRIBUTE_DIRECTORY);
    EMIT_H("create-new-with-directory-attribute", h); CLOSE(h);
    wd_emit("attr-of-new-with-directory-attribute", "%s", wd_attr(L"n3"));
    h = wd_open(L"n4", GENERIC_WRITE, 0, CREATE_NEW, 0);
    CLOSE(h);
    wd_emit("attr-of-new-with-no-attribute", "%s", wd_attr(L"n4"));
    h = wd_open(L"n5", GENERIC_WRITE, 0, CREATE_NEW, FILE_ATTRIBUTE_NOT_CONTENT_INDEXED | FILE_ATTRIBUTE_OFFLINE);
    CLOSE(h);
    wd_emit("attr-of-new-not-indexed-offline", "%s", wd_attr(L"n5"));
    h = wd_open(L"n5", GENERIC_WRITE, 0, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL);
    CLOSE(h);
    wd_emit("attr-after-create-always-normal", "%s", wd_attr(L"n5"));
    wd_clean();

    /* directories through CreateFile */
    CreateDirectoryW(L"d", NULL);
    wd_mkfile(L"f", "x");
    h = wd_open(L"d", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("dir.open-without-backup-semantics", h); CLOSE(h);
    h = wd_open(L"d", 0, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("dir.open-no-access-without-backup-semantics", h); CLOSE(h);
    h = wd_open(L"d", GENERIC_READ | GENERIC_WRITE, WD_RWD, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS);
    EMIT_H("dir.open-read-write-backup-semantics", h);
    wd_emit("dir.GetFileType", "%lu", GetFileType(h));
    SetLastError(0xdeadbeef);
    n = GetFileSize(h, NULL);
    wd_emit("dir.GetFileSize", "%lu gle=%lu", n, n == INVALID_FILE_SIZE ? GetLastError() : 0);
    ok = wd_ok(ReadFile(h, buf, 1, &n, NULL));
    wd_emit("dir.ReadFile", "%s", wd_was());
    ok = wd_ok(WriteFile(h, "x", 1, &n, NULL));
    wd_emit("dir.WriteFile", "%s", wd_was());
    EMIT_B("dir.FlushFileBuffers", FlushFileBuffers(h));
    EMIT_B("dir.SetEndOfFile", SetEndOfFile(h));
    CLOSE(h);
    h = wd_open(L"d\\", GENERIC_READ, WD_RWD, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS);
    EMIT_H("dir.open-trailing-backslash", h); CLOSE(h);
    h = wd_open(L"d\\.", GENERIC_READ, WD_RWD, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS);
    EMIT_H("dir.open-trailing-dot-component", h); CLOSE(h);
    h = wd_open(L"f\\", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("file.open-trailing-backslash", h); CLOSE(h);
    h = wd_open(L"f\\.", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("file.open-trailing-dot-component", h); CLOSE(h);
    h = wd_open(L"f\\..\\f", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("file.open-through-file-dotdot", h); CLOSE(h);
    h = wd_open(L"f\\x", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("file.open-below-a-file", h); CLOSE(h);
    h = wd_open(L"f\\x", GENERIC_WRITE, WD_RWD, CREATE_NEW, 0);
    EMIT_H("file.create-below-a-file", h); CLOSE(h);
    h = wd_open(L"nodir\\x", GENERIC_WRITE, WD_RWD, CREATE_NEW, 0);
    EMIT_H("file.create-in-missing-directory", h); CLOSE(h);
    h = wd_open(L"", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("file.open-empty-name", h); CLOSE(h);
    h = CreateFileW(NULL, GENERIC_READ, WD_RWD, NULL, OPEN_EXISTING, 0, NULL);
    EMIT_H("file.open-null-name", h); CLOSE(h);
    EMIT_B("dir.CreateDirectory-exists", CreateDirectoryW(L"d", NULL));
    EMIT_B("dir.CreateDirectory-file-exists", CreateDirectoryW(L"f", NULL));
    EMIT_B("dir.CreateDirectory-missing-parent", CreateDirectoryW(L"nodir\\x", NULL));
    EMIT_B("dir.CreateDirectory-trailing-backslash", CreateDirectoryW(L"d2\\", NULL));
    EMIT_B("dir.CreateDirectory-empty", CreateDirectoryW(L"", NULL));
    EMIT_B("dir.CreateDirectory-dot", CreateDirectoryW(L".", NULL));
    EMIT_B("dir.CreateDirectory-below-file", CreateDirectoryW(L"f\\x", NULL));

    /* access and flag combinations */
    h = wd_open(L"f", 0, 0, OPEN_EXISTING, 0);
    EMIT_H("access.none", h);
    ok = wd_ok(ReadFile(h, buf, 1, &n, NULL));
    wd_emit("access.none.ReadFile", "%s", wd_was());
    wd_emit("access.none.GetFileSize", "%lu", GetFileSize(h, NULL));
    CLOSE(h);
    h = wd_open(L"f", MAXIMUM_ALLOWED, 0, OPEN_EXISTING, 0);
    EMIT_H("access.MAXIMUM_ALLOWED", h); CLOSE(h);
    h = wd_open(L"f", GENERIC_ALL, 0, OPEN_EXISTING, 0);
    EMIT_H("access.GENERIC_ALL", h); CLOSE(h);
    h = wd_open(L"f", 0x0c000000, 0, OPEN_EXISTING, 0);
    EMIT_H("access.reserved-bits", h); CLOSE(h);
    h = wd_open(L"f", ACCESS_SYSTEM_SECURITY, 0, OPEN_EXISTING, 0);
    EMIT_H("access.ACCESS_SYSTEM_SECURITY", h); CLOSE(h);
    h = wd_open(L"f", GENERIC_READ, 0x8, OPEN_EXISTING, 0);
    EMIT_H("share.invalid-bit", h); CLOSE(h);
    h = wd_open(L"f", GENERIC_READ, 0, OPEN_EXISTING, FILE_FLAG_OVERLAPPED);
    ok = wd_ok(ReadFile(h, buf, 1, &n, NULL));
    wd_emit("flags.overlapped-read-without-overlapped", "%s", wd_was());
    CLOSE(h);
    h = wd_open(L"f", GENERIC_READ, 0, OPEN_EXISTING, FILE_FLAG_NO_BUFFERING);
    EMIT_H("flags.no-buffering-open", h);
    ok = wd_ok(ReadFile(h, buf, 1, &n, NULL));
    wd_emit("flags.no-buffering-read-1-byte", "%s", wd_was());
    CLOSE(h);
    h = wd_open(L"f", GENERIC_READ, 0, OPEN_EXISTING, FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_SEQUENTIAL_SCAN | FILE_FLAG_RANDOM_ACCESS);
    EMIT_H("flags.reparse-sequential-random", h); CLOSE(h);
    h = wd_open(L"f", GENERIC_READ, 0, OPEN_EXISTING, FILE_FLAG_POSIX_SEMANTICS);
    EMIT_H("flags.posix-semantics", h); CLOSE(h);
    h = wd_open(L"F", GENERIC_READ, 0, OPEN_EXISTING, FILE_FLAG_POSIX_SEMANTICS);
    EMIT_H("flags.posix-semantics-other-case", h); CLOSE(h);
}

/* ---- attributes ------------------------------------------------------- */

static void g_attrs(void)
{
    static const DWORD values[] = {
        0, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM,
        FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_TEMPORARY, FILE_ATTRIBUTE_OFFLINE,
        FILE_ATTRIBUTE_NOT_CONTENT_INDEXED, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_COMPRESSED,
        FILE_ATTRIBUTE_SPARSE_FILE, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_ENCRYPTED, 0x40 /* DEVICE */,
        0x8000 /* INTEGRITY_STREAM */, 0x20000 /* NO_SCRUB_DATA */, 0x80000 /* PINNED */, 0x100000 /* UNPINNED */,
        FILE_ATTRIBUTE_READONLY | FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM,
        FILE_ATTRIBUTE_NORMAL | FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_NORMAL | FILE_ATTRIBUTE_ARCHIVE,
        0x80000000, 0x7fffffff,
    };
    unsigned i;
    char id[64], buf[8];
    HANDLE h;
    WIN32_FILE_ATTRIBUTE_DATA fad;
    DWORD n;
    BOOL ok;

    CreateDirectoryW(L"d", NULL);
    for (i = 0; i < ARRAY_SIZE(values); i++)
    {
        wd_mkfile(L"f", "x");
        _snprintf(id, sizeof(id), "file.set-0x%lx", values[i]);
        ok = wd_ok(SetFileAttributesW(L"f", values[i]));
        wd_emit(id, "%s then %s", wd_was(), wd_attr(L"f"));
        SetFileAttributesW(L"f", FILE_ATTRIBUTE_NORMAL);
        DeleteFileW(L"f");
    }
    for (i = 0; i < ARRAY_SIZE(values); i++)
    {
        if (values[i] & (FILE_ATTRIBUTE_COMPRESSED | FILE_ATTRIBUTE_ENCRYPTED | 0x80000000)) continue;
        _snprintf(id, sizeof(id), "dir.set-0x%lx", values[i]);
        ok = wd_ok(SetFileAttributesW(L"d", values[i]));
        wd_emit(id, "%s then %s", wd_was(), wd_attr(L"d"));
        SetFileAttributesW(L"d", FILE_ATTRIBUTE_DIRECTORY);
    }
    EMIT_B("set-on-missing", SetFileAttributesW(L"nofile", FILE_ATTRIBUTE_NORMAL));
    EMIT_B("set-on-missing-parent", SetFileAttributesW(L"nodir\\nofile", FILE_ATTRIBUTE_NORMAL));
    wd_emit("get-empty-name", "%s", wd_attr(L""));
    wd_emit("get-dot", "%s", wd_attr(L"."));
    wd_emit("get-dotdot-below-file", "%s", wd_attr(L"d\\..\\d"));
    wd_emit("get-wildcard", "%s", wd_attr(L"*"));
    wd_emit("get-trailing-backslash-dir", "%s", wd_attr(L"d\\"));
    wd_mkfile(L"f", "x");
    wd_emit("get-trailing-backslash-file", "%s", wd_attr(L"f\\"));
    wd_emit("get-root", "%s", wd_attr(L"C:\\"));
    wd_emit("get-drive-only", "%s", wd_attr(L"C:"));
    EMIT_B("GetFileAttributesEx-invalid-level", GetFileAttributesExW(L"f", (GET_FILEEX_INFO_LEVELS)1, &fad));
    ok = wd_ok(GetFileAttributesExW(L"f", GetFileExInfoStandard, &fad));
    wd_emit("GetFileAttributesEx", ok ? "ok a=0x%lx size=%lu" : "e=%lu", ok ? fad.dwFileAttributes : wd_last,
            fad.nFileSizeLow);
    ok = wd_ok(GetFileAttributesExW(L"d", GetFileExInfoStandard, &fad));
    wd_emit("GetFileAttributesEx-dir", ok ? "ok a=0x%lx size=%lu" : "e=%lu", ok ? fad.dwFileAttributes : wd_last,
            fad.nFileSizeLow);

    /* what read-only means */
    SetFileAttributesW(L"f", FILE_ATTRIBUTE_READONLY);
    h = wd_open(L"f", GENERIC_WRITE, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("readonly.open-GENERIC_WRITE", h); CLOSE(h);
    h = wd_open(L"f", FILE_WRITE_ATTRIBUTES, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("readonly.open-FILE_WRITE_ATTRIBUTES", h); CLOSE(h);
    h = wd_open(L"f", FILE_APPEND_DATA, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("readonly.open-FILE_APPEND_DATA", h); CLOSE(h);
    h = wd_open(L"f", DELETE, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("readonly.open-DELETE", h); CLOSE(h);
    h = wd_open(L"f", WRITE_DAC, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("readonly.open-WRITE_DAC", h); CLOSE(h);
    h = wd_open(L"f", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("readonly.open-GENERIC_READ", h);
    ok = wd_ok(WriteFile(h, "x", 1, &n, NULL));
    wd_emit("readonly.write-through-read-handle", "%s", wd_was());
    CLOSE(h);
    EMIT_B("readonly.MoveFile", MoveFileW(L"f", L"g"));
    EMIT_B("readonly.MoveFile-back", MoveFileW(L"g", L"f"));
    EMIT_B("readonly.clear", SetFileAttributesW(L"f", FILE_ATTRIBUTE_NORMAL));
    SetFileAttributesW(L"d", FILE_ATTRIBUTE_READONLY | FILE_ATTRIBUTE_DIRECTORY);
    EMIT_B("readonly-dir.create-file-inside", wd_mkfile(L"d\\x", "x"));
    EMIT_B("readonly-dir.delete-file-inside", DeleteFileW(L"d\\x"));
    EMIT_B("readonly-dir.RemoveDirectory", RemoveDirectoryW(L"d"));
    wd_emit("readonly-dir.attr-after", "%s", wd_attr(L"d"));
    SetFileAttributesW(L"d", FILE_ATTRIBUTE_DIRECTORY);
    (void)buf;
}

/* ---- times ------------------------------------------------------------ */

static void g_times(void)
{
    FILETIME c, a, w, t, t2, loc;
    SYSTEMTIME st;
    HANDLE h;
    WORD dd, dt;
    DWORD n;
    BOOL ok;
    static const struct { const char *n; int y, mo, d; } dates[] = {
        {"1601", 1601, 1, 2}, {"1969", 1969, 12, 31}, {"1970", 1970, 1, 1}, {"1980", 1980, 1, 1},
        {"2038", 2038, 1, 20}, {"2107", 2107, 12, 31}, {"2200", 2200, 6, 1}, {"9999", 9999, 12, 31},
    };
    unsigned i;
    char id[64];

    wd_mkfile(L"t", "x");
    h = wd_open(L"t", GENERIC_READ | GENERIC_WRITE, WD_RWD, OPEN_EXISTING, 0);
    for (i = 0; i < ARRAY_SIZE(dates); i++)
    {
        t = ft_of(dates[i].y, dates[i].mo, dates[i].d, 12);
        t.dwLowDateTime += 1234567;
        _snprintf(id, sizeof(id), "write-time.%s", dates[i].n);
        ok = wd_ok(SetFileTime(h, NULL, NULL, &t));
        memset(&w, 0, sizeof(w));
        GetFileTime(h, &c, &a, &w);
        wd_emit(id, "%s %s", wd_was(), CompareFileTime(&t, &w) ? ft_str(&w) : "roundtrip");
        _snprintf(id, sizeof(id), "creation-time.%s", dates[i].n);
        ok = wd_ok(SetFileTime(h, &t, NULL, NULL));
        memset(&c, 0, sizeof(c));
        GetFileTime(h, &c, &a, &w);
        wd_emit(id, "%s %s", wd_was(), CompareFileTime(&t, &c) ? ft_str(&c) : "roundtrip");
        _snprintf(id, sizeof(id), "access-time.%s", dates[i].n);
        ok = wd_ok(SetFileTime(h, NULL, &t, NULL));
        memset(&a, 0, sizeof(a));
        GetFileTime(h, &c, &a, &w);
        wd_emit(id, "%s %s", wd_was(), CompareFileTime(&t, &a) ? ft_str(&a) : "roundtrip");
    }
    t = ft_of(2001, 2, 3, 4);
    SetFileTime(h, &t, &t, &t);
    EMIT_B("set-all-null", SetFileTime(h, NULL, NULL, NULL));
    t2.dwLowDateTime = t2.dwHighDateTime = 0;
    ok = wd_ok(SetFileTime(h, NULL, NULL, &t2));
    GetFileTime(h, &c, &a, &w);
    wd_emit("set-zero-write-time", "%s %s", wd_was(), CompareFileTime(&t, &w) ? ft_str(&w) : "unchanged");
    t2.dwLowDateTime = t2.dwHighDateTime = 0xffffffff;
    ok = wd_ok(SetFileTime(h, NULL, NULL, &t2));
    WriteFile(h, "y", 1, &n, NULL);
    FlushFileBuffers(h);
    GetFileTime(h, &c, &a, &w);
    wd_emit("write-after-set-minus-one", "%s %s", wd_was(), CompareFileTime(&t, &w) ? "write time changed" : "unchanged");
    CLOSE(h);
    h = wd_open(L"t", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    GetFileTime(h, &c, &a, &w);
    wd_emit("write-time-after-reopen", "%s", CompareFileTime(&t, &w) ? "changed" : "unchanged");
    EMIT_B("set-through-read-handle", SetFileTime(h, NULL, NULL, &t));
    CLOSE(h);
    h = wd_open(L"t", FILE_WRITE_ATTRIBUTES, WD_RWD, OPEN_EXISTING, 0);
    EMIT_B("set-through-write-attributes-handle", SetFileTime(h, NULL, NULL, &t));
    CLOSE(h);

    /* creation time of a name that is deleted and created again at once */
    h = wd_open(L"tun", GENERIC_WRITE, WD_RWD, CREATE_NEW, 0);
    SetFileTime(h, &t, NULL, NULL);
    CLOSE(h);
    DeleteFileW(L"tun");
    h = wd_open(L"tun", GENERIC_WRITE, WD_RWD, CREATE_NEW, 0);
    GetFileTime(h, &c, &a, &w);
    wd_emit("creation-time-of-recreated-name", "%s", CompareFileTime(&t, &c) ? "new" : "kept");
    CLOSE(h);
    /* and of a file that is overwritten */
    h = wd_open(L"t", GENERIC_WRITE, WD_RWD, CREATE_ALWAYS, 0);
    GetFileTime(h, &c, &a, &w);
    wd_emit("creation-time-after-create-always", "%s", CompareFileTime(&t, &c) ? "new" : "kept");
    CLOSE(h);

    /* conversions */
    t.dwLowDateTime = 0; t.dwHighDateTime = 0x80000000;
    EMIT_B("FileTimeToSystemTime-high-bit", FileTimeToSystemTime(&t, &st));
    t.dwLowDateTime = 0xffffffff; t.dwHighDateTime = 0x7fffffff;
    ok = wd_ok(FileTimeToSystemTime(&t, &st));
    wd_emit("FileTimeToSystemTime-max", ok ? "ok year=%u" : "e=%lu", ok ? (unsigned)st.wYear : (unsigned)wd_last);
    t.dwLowDateTime = 0; t.dwHighDateTime = 0;
    ok = wd_ok(FileTimeToSystemTime(&t, &st));
    wd_emit("FileTimeToSystemTime-zero", ok ? "ok %u-%u-%u dow=%u" : "e=%lu", ok ? (unsigned)st.wYear : (unsigned)wd_last,
            st.wMonth, st.wDay, st.wDayOfWeek);
    memset(&st, 0, sizeof(st)); st.wYear = 2024; st.wMonth = 13; st.wDay = 1;
    EMIT_B("SystemTimeToFileTime-month-13", SystemTimeToFileTime(&st, &t));
    st.wMonth = 2; st.wDay = 30;
    EMIT_B("SystemTimeToFileTime-feb-30", SystemTimeToFileTime(&st, &t));
    st.wDay = 29;
    EMIT_B("SystemTimeToFileTime-feb-29-leap", SystemTimeToFileTime(&st, &t));
    st.wYear = 1600; st.wMonth = 12; st.wDay = 31;
    EMIT_B("SystemTimeToFileTime-1600", SystemTimeToFileTime(&st, &t));
    st.wYear = 30828; st.wMonth = 1; st.wDay = 1;
    EMIT_B("SystemTimeToFileTime-30828", SystemTimeToFileTime(&st, &t));
    st.wYear = 30827; st.wMonth = 12; st.wDay = 31; st.wHour = 24;
    EMIT_B("SystemTimeToFileTime-hour-24", SystemTimeToFileTime(&st, &t));
    st.wYear = 2024; st.wHour = 0; st.wMilliseconds = 1000;
    EMIT_B("SystemTimeToFileTime-ms-1000", SystemTimeToFileTime(&st, &t));
    st.wMilliseconds = 0; st.wDayOfWeek = 99;
    EMIT_B("SystemTimeToFileTime-bad-day-of-week", SystemTimeToFileTime(&st, &t));

    t = ft_of(1979, 12, 31, 23);
    EMIT_B("FileTimeToDosDateTime-1979", FileTimeToDosDateTime(&t, &dd, &dt));
    t = ft_of(1980, 1, 1, 0);
    ok = wd_ok(FileTimeToDosDateTime(&t, &dd, &dt));
    wd_emit("FileTimeToDosDateTime-1980", ok ? "ok %04x %04x" : "e=%lu", ok ? (unsigned)dd : (unsigned)wd_last, dt);
    t = ft_of(2107, 12, 31, 23);
    ok = wd_ok(FileTimeToDosDateTime(&t, &dd, &dt));
    wd_emit("FileTimeToDosDateTime-2107", ok ? "ok %04x %04x" : "e=%lu", ok ? (unsigned)dd : (unsigned)wd_last, dt);
    t = ft_of(2108, 1, 1, 0);
    EMIT_B("FileTimeToDosDateTime-2108", FileTimeToDosDateTime(&t, &dd, &dt));
    t = ft_of(2001, 2, 3, 4);
    t.dwLowDateTime += 10000000; /* an odd second */
    ok = wd_ok(FileTimeToDosDateTime(&t, &dd, &dt));
    wd_emit("FileTimeToDosDateTime-odd-second", ok ? "ok %04x %04x" : "e=%lu", ok ? (unsigned)dd : (unsigned)wd_last, dt);
    ok = wd_ok(DosDateTimeToFileTime(0x0021, 0, &t));
    wd_emit("DosDateTimeToFileTime-1980-01-01", "%s %s", wd_was(), ok ? ft_str(&t) : "");
    EMIT_B("DosDateTimeToFileTime-zero-date", DosDateTimeToFileTime(0, 0, &t));
    EMIT_B("DosDateTimeToFileTime-month-0", DosDateTimeToFileTime(0x0001, 0, &t));
    EMIT_B("DosDateTimeToFileTime-month-13", DosDateTimeToFileTime(0x01a1, 0, &t));
    EMIT_B("DosDateTimeToFileTime-hour-24", DosDateTimeToFileTime(0x0021, 0xc000, &t));
    EMIT_B("DosDateTimeToFileTime-second-60", DosDateTimeToFileTime(0x0021, 0x001e, &t));
    ok = wd_ok(DosDateTimeToFileTime(0xff9f, 0xbf7d, &t));
    wd_emit("DosDateTimeToFileTime-max", "%s %s", wd_was(), ok ? ft_str(&t) : "");
    t.dwLowDateTime = 0; t.dwHighDateTime = 0;
    ok = wd_ok(FileTimeToLocalFileTime(&t, &loc));
    wd_emit("FileTimeToLocalFileTime-zero", "%s", wd_was());
    t.dwLowDateTime = 0xffffffff; t.dwHighDateTime = 0x7fffffff;
    ok = wd_ok(LocalFileTimeToFileTime(&t, &loc));
    wd_emit("LocalFileTimeToFileTime-max", "%s", wd_was());
}

/* ---- hard links and file identity ------------------------------------- */

static void g_links(void)
{
    HANDLE h, h2;
    BY_HANDLE_FILE_INFORMATION a, b;
    WCHAR buf[700];
    DWORD n;
    BOOL ok;
    FILE_STANDARD_INFO si;
    struct { DWORD len; WCHAR name[400]; } ni;
    unsigned i;
    WCHAR *p;

    wd_mkfile(L"a", "hello");
    CreateDirectoryW(L"d", NULL);
    EMIT_B("CreateHardLink", CreateHardLinkW(L"b", L"a", NULL));
    EMIT_B("CreateHardLink-target-exists", CreateHardLinkW(L"b", L"a", NULL));
    EMIT_B("CreateHardLink-source-missing", CreateHardLinkW(L"c", L"nofile", NULL));
    EMIT_B("CreateHardLink-to-directory", CreateHardLinkW(L"c", L"d", NULL));
    EMIT_B("CreateHardLink-missing-parent", CreateHardLinkW(L"nodir\\c", L"a", NULL));
    h = wd_open(L"a", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    h2 = wd_open(L"b", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    memset(&a, 0, sizeof(a)); memset(&b, 0, sizeof(b));
    GetFileInformationByHandle(h, &a);
    GetFileInformationByHandle(h2, &b);
    wd_emit("links-count", "a=%lu b=%lu same-index=%d same-volume=%d", a.nNumberOfLinks, b.nNumberOfLinks,
            a.nFileIndexLow == b.nFileIndexLow && a.nFileIndexHigh == b.nFileIndexHigh,
            a.dwVolumeSerialNumber == b.dwVolumeSerialNumber);
    SetFileAttributesW(L"a", FILE_ATTRIBUTE_HIDDEN);
    wd_emit("attributes-shared-between-links", "%s", wd_attr(L"b"));
    SetFileAttributesW(L"a", FILE_ATTRIBUTE_NORMAL);
    EMIT_B("delete-one-link-while-open", DeleteFileW(L"a"));
    GetFileInformationByHandle(h2, &b);
    wd_emit("links-after-delete", "b=%lu", b.nNumberOfLinks);
    ok = wd_ok(GetFileInformationByHandleEx(h2, FileStandardInfo, &si, sizeof(si)));
    wd_emit("FileStandardInfo", ok ? "ok links=%lu pending=%d dir=%d eof=%lu" : "e=%lu",
            ok ? si.NumberOfLinks : wd_last, si.DeletePending, si.Directory, si.EndOfFile.LowPart);
    memset(&ni, 0, sizeof(ni));
    ok = wd_ok(GetFileInformationByHandleEx(h2, FileNameInfo, &ni, sizeof(ni)));
    wd_emit("FileNameInfo", ok ? "ok len=%lu %s" : "e=%lu", ok ? ni.len : wd_last, ok ? wd_u8n(ni.name, ni.len / 2) : "");
    ok = wd_ok(GetFileInformationByHandleEx(h2, FileNameInfo, &ni, 8));
    wd_emit("FileNameInfo-small-buffer", "%s", wd_was());
    ok = wd_ok(GetFileInformationByHandleEx(h2, (FILE_INFO_BY_HANDLE_CLASS)99, &ni, sizeof(ni)));
    wd_emit("GetFileInformationByHandleEx-bad-class", "%s", wd_was());
    {
        static const struct { const char *n; DWORD f; } fl[] = {
            {"dos", 0}, {"none", 4 /* VOLUME_NAME_NONE */}, {"guid", 1}, {"nt", 2}, {"opened", 8}, {"bad", 0x40},
        };
        for (i = 0; i < ARRAY_SIZE(fl); i++)
        {
            char id[64];
            _snprintf(id, sizeof(id), "GetFinalPathNameByHandle.%s", fl[i].n);
            buf[0] = 0;
            n = GetFinalPathNameByHandleW(h2, buf, 690, fl[i].f);
            if (!n) { wd_emit(id, "e=%lu", GetLastError()); continue; }
            if (fl[i].f == 1) { wd_emit(id, "len=%lu prefix=%s", n, wd_u8n(buf, 11)); continue; }
            /* \Device\HarddiskVolume3 differs between machines */
            if (fl[i].f == 2 && (p = wcsstr(buf, L"\\wd\\"))) { wd_emit(id, "%s", wd_u8(p)); continue; }
            wd_emit(id, "len=%lu %s", n, wd_path(buf));
        }
        n = GetFinalPathNameByHandleW(h2, buf, 5, 0);
        wd_emit("GetFinalPathNameByHandle.small-buffer", "ret-minus-len=%ld gle=%lu", (long)n - (long)lstrlenW(wd_cwd), GetLastError());
    }
    CLOSE(h); CLOSE(h2);
    h = wd_open(L"d", GENERIC_READ, WD_RWD, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS);
    memset(&a, 0, sizeof(a));
    ok = wd_ok(GetFileInformationByHandle(h, &a));
    wd_emit("dir.GetFileInformationByHandle", ok ? "ok links=%lu a=0x%lx size=%lu" : "e=%lu",
            ok ? a.nNumberOfLinks : wd_last, a.dwFileAttributes, a.nFileSizeLow);
    CLOSE(h);
    SetLastError(0xdeadbeef);
    n = GetFileType(INVALID_HANDLE_VALUE);
    wd_emit("GetFileType-invalid", "%lu gle=%lu", n, GetLastError());
    n = GetCompressedFileSizeW(L"b", NULL);
    wd_emit("GetCompressedFileSize", "%lu", n);
    SetLastError(0xdeadbeef);
    n = GetCompressedFileSizeW(L"nofile", NULL);
    wd_emit("GetCompressedFileSize-missing", "%lu gle=%lu", n, GetLastError());
}

/* ---- renaming and copying --------------------------------------------- */

static void g_rename(void)
{
    HANDLE h, h2;
    char buf[8];
    DWORD n;
    BOOL ok;
    FILETIME t, w, c, a;
    WCHAR full[700];

    wd_mkfile(L"a", "A"); wd_mkfile(L"b", "B");
    EMIT_B("MoveFile-target-exists", MoveFileW(L"a", L"b"));
    EMIT_B("MoveFileEx-replace", MoveFileExW(L"a", L"b", MOVEFILE_REPLACE_EXISTING));
    wd_emit("listing-after-replace", "%s", wd_ls(L"."));
    wd_mkfile(L"a", "A");
    SetFileAttributesW(L"b", FILE_ATTRIBUTE_READONLY);
    EMIT_B("MoveFileEx-replace-readonly-target", MoveFileExW(L"a", L"b", MOVEFILE_REPLACE_EXISTING));
    SetFileAttributesW(L"b", FILE_ATTRIBUTE_HIDDEN);
    EMIT_B("MoveFileEx-replace-hidden-target", MoveFileExW(L"a", L"b", MOVEFILE_REPLACE_EXISTING));
    wd_clean();

    wd_mkfile(L"a", "A"); wd_mkfile(L"b", "B");
    h = wd_open(L"b", GENERIC_READ, WD_RW, OPEN_EXISTING, 0);
    EMIT_B("MoveFileEx-replace-target-open-share-rw", MoveFileExW(L"a", L"b", MOVEFILE_REPLACE_EXISTING));
    CLOSE(h);
    h = wd_open(L"b", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_B("MoveFileEx-replace-target-open-share-rwd", MoveFileExW(L"a", L"b", MOVEFILE_REPLACE_EXISTING));
    ok = wd_ok(ReadFile(h, buf, 1, &n, NULL));
    wd_emit("read-replaced-target-through-old-handle", ok && n == 1 ? "ok '%c'" : "failed", buf[0]);
    CLOSE(h);
    wd_clean();

    wd_mkfile(L"a", "A");
    h = wd_open(L"a", GENERIC_READ, WD_RW, OPEN_EXISTING, 0);
    EMIT_B("MoveFile-source-open-share-rw", MoveFileW(L"a", L"c"));
    CLOSE(h);
    h = wd_open(L"a", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_B("MoveFile-source-open-share-rwd", MoveFileW(L"a", L"c"));
    n = GetFinalPathNameByHandleW(h, full, 690, 0);
    wd_emit("name-of-open-handle-after-rename", n ? "%s" : "e=%lu", n ? wd_path(full) : (const char *)(ULONG_PTR)GetLastError());
    CLOSE(h);
    wd_clean();

    CreateDirectoryW(L"d", NULL); wd_mkfile(L"d\\f", "x"); CreateDirectoryW(L"d\\sub", NULL);
    CreateDirectoryW(L"e", NULL); wd_mkfile(L"a", "A");
    EMIT_B("MoveFile-directory", MoveFileW(L"d", L"d2"));
    MoveFileW(L"d2", L"d");
    h = wd_open(L"d\\f", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_B("MoveFile-directory-with-open-file", MoveFileW(L"d", L"d2"));
    CLOSE(h);
    MoveFileW(L"d2", L"d");
    h = wd_open(L"d\\sub", GENERIC_READ, WD_RWD, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS);
    EMIT_B("MoveFile-directory-with-open-subdirectory", MoveFileW(L"d", L"d2"));
    CLOSE(h);
    MoveFileW(L"d2", L"d");
    h = wd_open(L"d", GENERIC_READ, WD_RWD, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS);
    EMIT_B("MoveFile-open-directory-share-rwd", MoveFileW(L"d", L"d2"));
    CLOSE(h);
    MoveFileW(L"d2", L"d");
    EMIT_B("MoveFile-directory-onto-directory", MoveFileW(L"d", L"e"));
    EMIT_B("MoveFileEx-replace-directory-onto-empty-directory", MoveFileExW(L"d", L"e", MOVEFILE_REPLACE_EXISTING));
    EMIT_B("MoveFileEx-replace-file-onto-directory", MoveFileExW(L"a", L"e", MOVEFILE_REPLACE_EXISTING));
    EMIT_B("MoveFileEx-replace-directory-onto-file", MoveFileExW(L"d", L"a", MOVEFILE_REPLACE_EXISTING));
    EMIT_B("MoveFile-directory-into-itself", MoveFileW(L"d", L"d\\sub\\d"));
    EMIT_B("MoveFile-same-name", MoveFileW(L"a", L"a"));
    EMIT_B("MoveFile-missing-source", MoveFileW(L"nofile", L"x"));
    EMIT_B("MoveFile-missing-target-directory", MoveFileW(L"a", L"nodir\\x"));
    EMIT_B("MoveFile-empty-target", MoveFileW(L"a", L""));
    EMIT_B("MoveFile-empty-source", MoveFileW(L"", L"x"));
    EMIT_B("MoveFile-target-trailing-backslash", MoveFileW(L"a", L"x\\"));
    EMIT_B("MoveFile-into-directory-name", MoveFileW(L"a", L"e\\"));
    EMIT_B("MoveFileEx-invalid-flag", MoveFileExW(L"a", L"x", 0x1000));
    EMIT_B("MoveFileEx-copy-allowed-write-through", MoveFileExW(L"a", L"x", MOVEFILE_COPY_ALLOWED | MOVEFILE_WRITE_THROUGH));
    wd_emit("listing-at-end", "%s", wd_ls(L"."));
    wd_clean();
    wd_mkfile(L"a", "A");
    EMIT_B("MoveFile-null-target", MoveFileW(L"a", NULL));
    EMIT_B("MoveFileEx-null-target-no-flags", MoveFileExW(L"a", NULL, 0));
    wd_emit("listing-after-null-target", "%s", wd_ls(L"."));
    wd_clean();

    /* CopyFile */
    wd_mkfile(L"a", "AAAA"); wd_mkfile(L"b", "B"); CreateDirectoryW(L"d", NULL);
    EMIT_B("CopyFile-fail-if-exists", CopyFileW(L"a", L"b", TRUE));
    EMIT_B("CopyFile-overwrite", CopyFileW(L"a", L"b", FALSE));
    EMIT_B("CopyFile-onto-itself", CopyFileW(L"a", L"a", FALSE));
    EMIT_B("CopyFile-missing-source", CopyFileW(L"nofile", L"x", FALSE));
    EMIT_B("CopyFile-directory-source", CopyFileW(L"d", L"x", FALSE));
    EMIT_B("CopyFile-directory-target", CopyFileW(L"a", L"d", FALSE));
    EMIT_B("CopyFile-missing-target-directory", CopyFileW(L"a", L"nodir\\x", FALSE));
    SetFileAttributesW(L"b", FILE_ATTRIBUTE_READONLY);
    EMIT_B("CopyFile-onto-readonly", CopyFileW(L"a", L"b", FALSE));
    SetFileAttributesW(L"b", FILE_ATTRIBUTE_HIDDEN);
    EMIT_B("CopyFile-onto-hidden", CopyFileW(L"a", L"b", FALSE));
    SetFileAttributesW(L"b", FILE_ATTRIBUTE_NORMAL);
    h = wd_open(L"b", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_B("CopyFile-onto-open-share-rwd", CopyFileW(L"a", L"b", FALSE));
    CLOSE(h);
    h = wd_open(L"b", GENERIC_READ, FILE_SHARE_READ, OPEN_EXISTING, 0);
    EMIT_B("CopyFile-onto-open-share-r", CopyFileW(L"a", L"b", FALSE));
    CLOSE(h);
    h = wd_open(L"a", GENERIC_WRITE, FILE_SHARE_READ, OPEN_EXISTING, 0);
    EMIT_B("CopyFile-source-open-for-write-share-r", CopyFileW(L"a", L"c1", FALSE));
    CLOSE(h);
    h = wd_open(L"a", GENERIC_WRITE, WD_RW, OPEN_EXISTING, 0);
    EMIT_B("CopyFile-source-open-for-write-share-rw", CopyFileW(L"a", L"c2", FALSE));
    CLOSE(h);
    h = wd_open(L"a", GENERIC_READ, 0, OPEN_EXISTING, 0);
    EMIT_B("CopyFile-source-open-share-none", CopyFileW(L"a", L"c3", FALSE));
    CLOSE(h);
    SetFileAttributesW(L"a", FILE_ATTRIBUTE_READONLY | FILE_ATTRIBUTE_HIDDEN);
    t = ft_of(2001, 2, 3, 4);
    h = wd_open(L"a", FILE_WRITE_ATTRIBUTES, WD_RWD, OPEN_EXISTING, 0);
    SetFileTime(h, &t, NULL, &t);
    CLOSE(h);
    EMIT_B("CopyFile-readonly-hidden-source", CopyFileW(L"a", L"c4", FALSE));
    wd_emit("CopyFile-copies-attributes", "%s", wd_attr(L"c4"));
    h2 = wd_open(L"c4", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    memset(&w, 0, sizeof(w)); memset(&c, 0, sizeof(c));
    GetFileTime(h2, &c, &a, &w);
    wd_emit("CopyFile-copies-times", "write=%s creation=%s", CompareFileTime(&t, &w) ? "new" : "kept",
            CompareFileTime(&t, &c) ? "new" : "kept");
    CLOSE(h2);
    wd_clean();

    /* ReplaceFile */
    wd_mkfile(L"old", "OLD"); wd_mkfile(L"new", "NEW");
    EMIT_B("ReplaceFile-with-backup", ReplaceFileW(L"old", L"new", L"bak", 0, NULL, NULL));
    wd_emit("ReplaceFile-listing", "%s", wd_ls(L"."));
    wd_mkfile(L"new", "NEW");
    EMIT_B("ReplaceFile-no-backup", ReplaceFileW(L"old", L"new", NULL, 0, NULL, NULL));
    EMIT_B("ReplaceFile-missing-replacement", ReplaceFileW(L"old", L"new", NULL, 0, NULL, NULL));
    wd_mkfile(L"new", "NEW");
    EMIT_B("ReplaceFile-missing-replaced", ReplaceFileW(L"nofile", L"new", NULL, 0, NULL, NULL));
    SetFileAttributesW(L"old", FILE_ATTRIBUTE_READONLY);
    EMIT_B("ReplaceFile-readonly-replaced", ReplaceFileW(L"old", L"new", NULL, 0, NULL, NULL));
    SetFileAttributesW(L"old", FILE_ATTRIBUTE_NORMAL);
    h = wd_open(L"old", GENERIC_READ, WD_RW, OPEN_EXISTING, 0);
    EMIT_B("ReplaceFile-replaced-open-share-rw", ReplaceFileW(L"old", L"new", NULL, 0, NULL, NULL));
    CLOSE(h);
    wd_emit("ReplaceFile-listing-at-end", "%s", wd_ls(L"."));
}

/* ---- reading, writing, seeking, locking ------------------------------- */

static void g_io(void)
{
    HANDLE h, h2;
    char buf[64];
    DWORD n, pos;
    LARGE_INTEGER li, out;
    OVERLAPPED ov;
    BOOL ok;
    LONG high;

    h = wd_open(L"io", GENERIC_READ | GENERIC_WRITE, WD_RWD, CREATE_NEW, 0);
    n = 99; ok = wd_ok(WriteFile(h, "x", 0, &n, NULL));
    wd_emit("write-0-bytes", "%s n=%lu", wd_was(), n);
    n = 99; ok = wd_ok(ReadFile(h, buf, 10, &n, NULL));
    wd_emit("read-at-end", "%s n=%lu", wd_was(), n);
    n = 99; ok = wd_ok(ReadFile(h, NULL, 0, &n, NULL));
    wd_emit("read-0-bytes-null-buffer", "%s n=%lu", wd_was(), n);
    SetLastError(0xdeadbeef);
    pos = SetFilePointer(h, -1, NULL, FILE_BEGIN);
    wd_emit("seek-negative", "%lx gle=%lu", pos, GetLastError());
    SetLastError(0xdeadbeef);
    high = -1;
    pos = SetFilePointer(h, -1, &high, FILE_BEGIN);
    wd_emit("seek-negative-64", "%lx gle=%lu", pos, GetLastError());
    SetLastError(0xdeadbeef);
    pos = SetFilePointer(h, 10, NULL, FILE_END);
    wd_emit("seek-past-end", "%lu size=%lu", pos, GetFileSize(h, NULL));
    ok = wd_ok(WriteFile(h, "Z", 1, &n, NULL));
    wd_emit("write-past-end", "%s size=%lu", wd_was(), GetFileSize(h, NULL));
    SetFilePointer(h, 0, NULL, FILE_BEGIN);
    memset(buf, 'q', sizeof(buf));
    ok = wd_ok(ReadFile(h, buf, 20, &n, NULL));
    wd_emit("read-the-gap", "%s n=%lu first=%d last=%c", wd_was(), n, buf[0], buf[10]);
    SetLastError(0xdeadbeef);
    pos = SetFilePointer(h, 0, NULL, 3);
    wd_emit("seek-bad-method", "%lx gle=%lu", pos, GetLastError());
    li.QuadPart = 5;
    ok = wd_ok(SetFilePointerEx(h, li, &out, 7));
    wd_emit("SetFilePointerEx-bad-method", "%s", wd_was());
    li.QuadPart = -5;
    ok = wd_ok(SetFilePointerEx(h, li, &out, FILE_BEGIN));
    wd_emit("SetFilePointerEx-negative", "%s", wd_was());
    li.QuadPart = 0x7fffffffffffffffLL;
    ok = wd_ok(SetFilePointerEx(h, li, &out, FILE_BEGIN));
    wd_emit("SetFilePointerEx-max", "%s", wd_was());
    EMIT_B("SetEndOfFile-at-max", SetEndOfFile(h));
    ok = wd_ok(WriteFile(h, "x", 1, &n, NULL));
    wd_emit("write-at-max", "%s", wd_was());
    SetFilePointer(h, 3, NULL, FILE_BEGIN);
    EMIT_B("SetEndOfFile-truncate", SetEndOfFile(h));
    wd_emit("size-after-truncate", "%lu", GetFileSize(h, NULL));
    SetFilePointer(h, 0x100000, NULL, FILE_BEGIN);
    EMIT_B("SetEndOfFile-extend", SetEndOfFile(h));
    wd_emit("size-after-extend", "%lu", GetFileSize(h, NULL));
    SetFilePointer(h, 0, NULL, FILE_BEGIN); SetEndOfFile(h);
    WriteFile(h, "0123456789", 10, &n, NULL);

    /* locks */
    EMIT_B("LockFile", LockFile(h, 2, 0, 4, 0));
    EMIT_B("LockFile-same-range-same-handle", LockFile(h, 2, 0, 4, 0));
    EMIT_B("LockFile-overlapping-same-handle", LockFile(h, 4, 0, 4, 0));
    EMIT_B("LockFile-adjacent-same-handle", LockFile(h, 6, 0, 2, 0));
    EMIT_B("LockFile-zero-length", LockFile(h, 3, 0, 0, 0));
    EMIT_B("LockFile-beyond-end", LockFile(h, 1000, 0, 10, 0));
    h2 = wd_open(L"io", GENERIC_READ | GENERIC_WRITE, WD_RWD, OPEN_EXISTING, 0);
    SetFilePointer(h2, 0, NULL, FILE_BEGIN);
    ok = wd_ok(ReadFile(h2, buf, 2, &n, NULL));
    wd_emit("read-before-locked-range", "%s n=%lu", wd_was(), ok ? n : 0);
    SetFilePointer(h2, 0, NULL, FILE_BEGIN);
    ok = wd_ok(ReadFile(h2, buf, 4, &n, NULL));
    wd_emit("read-into-locked-range", "%s", wd_was());
    SetFilePointer(h2, 3, NULL, FILE_BEGIN);
    ok = wd_ok(WriteFile(h2, "w", 1, &n, NULL));
    wd_emit("write-locked-range-other-handle", "%s", wd_was());
    SetFilePointer(h, 3, NULL, FILE_BEGIN);
    ok = wd_ok(WriteFile(h, "w", 1, &n, NULL));
    wd_emit("write-locked-range-owner-handle", "%s", wd_was());
    EMIT_B("LockFile-other-handle-conflict", LockFile(h2, 3, 0, 1, 0));
    EMIT_B("UnlockFile-other-handle", UnlockFile(h2, 2, 0, 4, 0));
    EMIT_B("UnlockFile-part-of-range", UnlockFile(h, 2, 0, 2, 0));
    EMIT_B("UnlockFile-not-locked", UnlockFile(h, 20, 0, 2, 0));
    EMIT_B("UnlockFile", UnlockFile(h, 2, 0, 4, 0));
    EMIT_B("UnlockFile-twice", UnlockFile(h, 2, 0, 4, 0));
    memset(&ov, 0, sizeof(ov)); ov.Offset = 100;
    EMIT_B("LockFileEx-shared", LockFileEx(h, 0, 0, 10, 0, &ov));
    EMIT_B("LockFileEx-shared-other-handle", LockFileEx(h2, LOCKFILE_FAIL_IMMEDIATELY, 0, 10, 0, &ov));
    EMIT_B("LockFileEx-exclusive-over-shared", LockFileEx(h2, LOCKFILE_FAIL_IMMEDIATELY | LOCKFILE_EXCLUSIVE_LOCK, 0, 10, 0, &ov));
    EMIT_B("LockFileEx-reserved-nonzero", LockFileEx(h, LOCKFILE_FAIL_IMMEDIATELY, 1, 10, 0, &ov));
    EMIT_B("SetEndOfFile-with-lock-held-by-other", (SetFilePointer(h2, 5, NULL, FILE_BEGIN), SetEndOfFile(h2)));
    EMIT_B("DeleteFile-with-locks", DeleteFileW(L"io"));
    CLOSE(h2);
    h2 = wd_open(L"io", GENERIC_READ | GENERIC_WRITE, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("reopen-after-delete-with-locks", h2);
    CLOSE(h2);
    CLOSE(h);
    wd_clean();

    /* access checks on handles */
    wd_mkfile(L"f", "0123456789");
    h = wd_open(L"f", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    ok = wd_ok(WriteFile(h, "x", 1, &n, NULL));
    wd_emit("write-through-read-handle", "%s", wd_was());
    EMIT_B("FlushFileBuffers-read-handle", FlushFileBuffers(h));
    EMIT_B("SetEndOfFile-read-handle", SetEndOfFile(h));
    EMIT_B("LockFile-read-handle", LockFile(h, 0, 0, 1, 0));
    CLOSE(h);
    h = wd_open(L"f", GENERIC_WRITE, WD_RWD, OPEN_EXISTING, 0);
    ok = wd_ok(ReadFile(h, buf, 1, &n, NULL));
    wd_emit("read-through-write-handle", "%s", wd_was());
    EMIT_B("FlushFileBuffers-write-handle", FlushFileBuffers(h));
    wd_emit("GetFileSize-write-handle", "%lu", GetFileSize(h, NULL));
    CLOSE(h);
    h = wd_open(L"f", FILE_APPEND_DATA, WD_RWD, OPEN_EXISTING, 0);
    SetFilePointer(h, 0, NULL, FILE_BEGIN);
    ok = wd_ok(WriteFile(h, "AP", 2, &n, NULL));
    wd_emit("append-handle-write-at-0", "%s size=%lu", wd_was(), GetFileSize(h, NULL));
    EMIT_B("append-handle-SetEndOfFile", (SetFilePointer(h, 2, NULL, FILE_BEGIN), SetEndOfFile(h)));
    CLOSE(h);
    h = wd_open(L"f", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    memset(buf, 0, sizeof(buf));
    ReadFile(h, buf, 20, &n, NULL);
    wd_emit("content-after-append", "n=%lu %s", n, buf);
    CLOSE(h);

    /* invalid handles */
    ok = wd_ok(WriteFile(INVALID_HANDLE_VALUE, "x", 1, &n, NULL));
    wd_emit("WriteFile-invalid-handle", "%s", wd_was());
    ok = wd_ok(ReadFile(NULL, buf, 1, &n, NULL));
    wd_emit("ReadFile-null-handle", "%s", wd_was());
    EMIT_B("CloseHandle-null", CloseHandle(NULL));
    EMIT_B("CloseHandle-invalid", CloseHandle(INVALID_HANDLE_VALUE));
    EMIT_B("FlushFileBuffers-invalid", FlushFileBuffers(INVALID_HANDLE_VALUE));
    SetLastError(0xdeadbeef);
    n = GetFileSize(INVALID_HANDLE_VALUE, NULL);
    wd_emit("GetFileSize-invalid", "%lx gle=%lu", n, GetLastError());
    SetLastError(0xdeadbeef);
    pos = SetFilePointer(INVALID_HANDLE_VALUE, 0, NULL, FILE_BEGIN);
    wd_emit("SetFilePointer-invalid", "%lx gle=%lu", pos, GetLastError());

    /* overlapped handles, synchronously awaited */
    h = wd_open(L"f", GENERIC_READ | GENERIC_WRITE, WD_RWD, OPEN_EXISTING, FILE_FLAG_OVERLAPPED);
    memset(&ov, 0, sizeof(ov)); ov.Offset = 1000;
    ok = wd_ok(ReadFile(h, buf, 4, &n, &ov));
    if (!ok && wd_last == ERROR_IO_PENDING) ok = wd_ok(GetOverlappedResult(h, &ov, &n, TRUE));
    wd_emit("overlapped-read-past-end", "%s", wd_was());
    memset(&ov, 0, sizeof(ov)); ov.Offset = 2;
    n = 0;
    ok = wd_ok(ReadFile(h, buf, 4, &n, &ov));
    if (!ok && wd_last == ERROR_IO_PENDING) ok = wd_ok(GetOverlappedResult(h, &ov, &n, TRUE));
    wd_emit("overlapped-read-at-2", "%s n=%lu", wd_was(), n);
    memset(&ov, 0, sizeof(ov)); ov.Offset = 8;
    n = 0;
    ok = wd_ok(ReadFile(h, buf, 40, &n, &ov));
    if (!ok && wd_last == ERROR_IO_PENDING) ok = wd_ok(GetOverlappedResult(h, &ov, &n, TRUE));
    wd_emit("overlapped-read-short", "%s n=%lu", wd_was(), n);
    wd_emit("overlapped-file-pointer", "%lu", SetFilePointer(h, 0, NULL, FILE_CURRENT));
    CLOSE(h);

    /* the NUL device */
    h = wd_open(L"NUL", GENERIC_READ | GENERIC_WRITE, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("nul.open", h);
    n = 99; ok = wd_ok(WriteFile(h, "hello", 5, &n, NULL));
    wd_emit("nul.write", "%s n=%lu", wd_was(), n);
    n = 99; ok = wd_ok(ReadFile(h, buf, 5, &n, NULL));
    wd_emit("nul.read", "%s n=%lu", wd_was(), n);
    wd_emit("nul.GetFileType", "%lu", GetFileType(h));
    SetLastError(0xdeadbeef);
    n = GetFileSize(h, NULL);
    wd_emit("nul.GetFileSize", "%lx gle=%lu", n, GetLastError());
    SetLastError(0xdeadbeef);
    pos = SetFilePointer(h, 10, NULL, FILE_BEGIN);
    wd_emit("nul.SetFilePointer", "%lx gle=%lu", pos, GetLastError());
    EMIT_B("nul.FlushFileBuffers", FlushFileBuffers(h));
    EMIT_B("nul.SetEndOfFile", SetEndOfFile(h));
    CLOSE(h);
    h = wd_open(L"NUL", GENERIC_WRITE, 0, CREATE_NEW, 0);
    EMIT_H("nul.create-new", h); CLOSE(h);
    h = wd_open(L"NUL", GENERIC_WRITE, 0, TRUNCATE_EXISTING, 0);
    EMIT_H("nul.truncate-existing", h); CLOSE(h);
}

/* ---- enumeration ------------------------------------------------------ */

static const char *find_all(const WCHAR *pattern, BOOL sorted)
{
    static WCHAR names[64][264];
    static WCHAR joined[4096];
    WIN32_FIND_DATAW fd;
    HANDLE h;
    int count = 0, i, j, n = 0;

    h = FindFirstFileW(pattern, &fd);
    if (h == INVALID_HANDLE_VALUE)
    {
        static char e[32];
        _snprintf(e, 32, "e=%lu", GetLastError());
        return e;
    }
    do
    {
        if (count >= 64) break;
        lstrcpynW(names[count++], fd.cFileName, 260);
    } while (FindNextFileW(h, &fd));
    FindClose(h);
    if (sorted)
        for (i = 1; i < count; i++)
            for (j = i; j > 0 && wcscmp(names[j - 1], names[j]) > 0; j--)
            {
                WCHAR t[264];
                memcpy(t, names[j], sizeof(t));
                memcpy(names[j], names[j - 1], sizeof(t));
                memcpy(names[j - 1], t, sizeof(t));
            }
    for (i = 0; i < count && n < 3700; i++)
    {
        int l = lstrlenW(names[i]);
        if (i) joined[n++] = '|';
        memcpy(joined + n, names[i], l * sizeof(WCHAR));
        n += l;
    }
    joined[n] = 0;
    return wd_u8(joined);
}

static void g_find(void)
{
    static const WCHAR *files[] = {
        L"a.txt", L"b.TXT", L"c", L"e.tar.gz", L".dot", L"sp ace.doc", L"abc.txtx", L"Zed.txt", L"_under.txt",
        L"10.txt", L"9.txt", L"B2.txt", L"longfilename.text", L"x.t", L"noext",
    };
    static const struct { const char *n; const WCHAR *p; } pat[] = {
        {"star", L"*"}, {"star-dot-star", L"*.*"}, {"star-dot", L"*."}, {"star-txt", L"*.txt"},
        {"star-TXT", L"*.TXT"}, {"star-t", L"*.t"}, {"star-t-q", L"*.t?"}, {"star-t-qqq", L"*.t???"},
        {"q-txt", L"?.txt"}, {"qq-txt", L"??.txt"}, {"a-star", L"a*"}, {"star-a-star", L"*a*"},
        {"star-txt-star", L"*.txt*"}, {"qqqq", L"????"}, {"q", L"?"}, {"c-dot", L"c."}, {"c-dot-star", L"c.*"},
        {"star-star-star", L"*.*.*"}, {"e-star", L"e.*"}, {"star-gz", L"*.gz"}, {"dot-star", L".*"},
        {"star-dot-dot", L"*.."}, {"noext-dot-dot", L"noext.."}, {"lt", L"<"}, {"lt-txt", L"<.txt"},
        {"gt", L">"}, {"gt-gt-txt", L">>.txt"}, {"quote-txt", L"*\"txt"}, {"lt-quote-star", L"<\"*"},
        {"star-space", L"* "}, {"trailing-space", L"a.txt "}, {"trailing-dot", L"a.txt."}, {"exact-other-case", L"A.TXT"},
        {"dir", L"d"}, {"dir-backslash", L"d\\"}, {"dir-star", L"d\\*"}, {"dot", L"."}, {"dotdot-star", L"..\\*.nomatch"},
        {"empty", L""}, {"missing", L"nofile"}, {"missing-dir", L"nodir\\*"}, {"below-file", L"a.txt\\*"},
        {"wildcard-directory", L"*\\a.txt"}, {"short-name-pattern", L"LONGFI~1.TEX"}, {"tilde", L"*~*"},
    };
    unsigned i;
    char id[80];
    WIN32_FIND_DATAW fd;
    HANDLE h;
    WCHAR buf[300];
    DWORD n;

    for (i = 0; i < ARRAY_SIZE(files); i++) wd_mkfile(files[i], "x");
    CreateDirectoryW(L"d", NULL);
    wd_mkfile(L"d\\inner", "x");
    for (i = 0; i < ARRAY_SIZE(pat); i++)
    {
        _snprintf(id, sizeof(id), "pattern.%s", pat[i].n);
        wd_emit(id, "%s", find_all(pat[i].p, TRUE));
    }
    wd_emit("order.star", "%s", find_all(L"*", FALSE));
    wd_emit("order.subdirectory", "%s", find_all(L"d\\*", FALSE));

    h = FindFirstFileW(L"longfilename.text", &fd);
    if (h != INVALID_HANDLE_VALUE)
    {
        wd_emit("short-name.find", "%s", fd.cAlternateFileName[0] ? wd_u8(fd.cAlternateFileName) : "(none)");
        wd_emit("find-data.file", "a=0x%lx size=%lu:%lu res0=%lu", fd.dwFileAttributes, fd.nFileSizeHigh, fd.nFileSizeLow, fd.dwReserved0);
        SetLastError(0xdeadbeef);
        wd_emit("FindNextFile-at-end", "%s", wd_res(FindNextFileW(h, &fd)));
        wd_emit("FindNextFile-at-end-again", "%s", wd_res(FindNextFileW(h, &fd)));
        FindClose(h);
    }
    h = FindFirstFileW(L"d", &fd);
    if (h != INVALID_HANDLE_VALUE)
    {
        wd_emit("find-data.directory", "a=0x%lx size=%lu:%lu res0=%lu", fd.dwFileAttributes, fd.nFileSizeHigh, fd.nFileSizeLow, fd.dwReserved0);
        FindClose(h);
    }
    h = FindFirstFileExW(L"longfilename.text", FindExInfoBasic, &fd, FindExSearchNameMatch, NULL, 0);
    if (h != INVALID_HANDLE_VALUE)
    {
        wd_emit("short-name.FindExInfoBasic", "%s", fd.cAlternateFileName[0] ? wd_u8(fd.cAlternateFileName) : "(none)");
        FindClose(h);
    }
    else wd_emit("short-name.FindExInfoBasic", "e=%lu", GetLastError());
    h = FindFirstFileExW(L"B.txt", FindExInfoStandard, &fd, FindExSearchNameMatch, NULL, FIND_FIRST_EX_CASE_SENSITIVE);
    EMIT_H("FindFirstFileEx-case-sensitive-other-case", h);
    if (h != INVALID_HANDLE_VALUE) FindClose(h);
    h = FindFirstFileExW(L"*", FindExInfoStandard, &fd, FindExSearchLimitToDirectories, NULL, 0);
    EMIT_H("FindFirstFileEx-limit-to-directories", h);
    if (h != INVALID_HANDLE_VALUE) FindClose(h);
    h = FindFirstFileExW(L"*", (FINDEX_INFO_LEVELS)2, &fd, FindExSearchNameMatch, NULL, 0);
    EMIT_H("FindFirstFileEx-bad-info-level", h);
    if (h != INVALID_HANDLE_VALUE) FindClose(h);
    h = FindFirstFileExW(L"*", FindExInfoStandard, &fd, (FINDEX_SEARCH_OPS)3, NULL, 0);
    EMIT_H("FindFirstFileEx-bad-search-op", h);
    if (h != INVALID_HANDLE_VALUE) FindClose(h);
    h = FindFirstFileExW(L"*", FindExInfoStandard, &fd, FindExSearchNameMatch, NULL, 0x100);
    EMIT_H("FindFirstFileEx-bad-flags", h);
    if (h != INVALID_HANDLE_VALUE) FindClose(h);
    EMIT_B("FindClose-invalid", FindClose(INVALID_HANDLE_VALUE));
    EMIT_B("FindClose-null", FindClose(NULL));
    EMIT_B("FindNextFile-invalid", FindNextFileW(INVALID_HANDLE_VALUE, &fd));

    buf[0] = 0;
    n = GetShortPathNameW(L"longfilename.text", buf, 290);
    wd_emit("short-name.GetShortPathName", n ? "%s" : "e=%lu", n ? wd_u8(buf) : (const char *)(ULONG_PTR)GetLastError());
    buf[0] = 0;
    n = GetShortPathNameW(L"sp ace.doc", buf, 290);
    wd_emit("short-name.with-space", n ? "%s" : "e=%lu", n ? wd_u8(buf) : (const char *)(ULONG_PTR)GetLastError());
    buf[0] = 0;
    n = GetShortPathNameW(L"a.txt", buf, 290);
    wd_emit("short-name.already-short", n ? "%s" : "e=%lu", n ? wd_u8(buf) : (const char *)(ULONG_PTR)GetLastError());
    n = GetShortPathNameW(L"nofile.text", buf, 290);
    wd_emit("short-name.missing", "%lu %s", n, wd_res(n != 0));
    buf[0] = 0;
    n = GetLongPathNameW(L"A.TXT", buf, 290);
    wd_emit("GetLongPathName-corrects-case", n ? "%s" : "e=%lu", n ? wd_u8(buf) : (const char *)(ULONG_PTR)GetLastError());
    buf[0] = 0;
    n = GetLongPathNameW(L".\\D\\INNER", buf, 290);
    wd_emit("GetLongPathName-relative", n ? "%s" : "e=%lu", n ? wd_u8(buf) : (const char *)(ULONG_PTR)GetLastError());
    n = GetLongPathNameW(L"nofile", buf, 290);
    wd_emit("GetLongPathName-missing", "%lu %s", n, wd_res(n != 0));
    n = GetLongPathNameW(L"a.txt", buf, 2);
    wd_emit("GetLongPathName-small-buffer", "%lu", n);
}

static const struct wd_group_def groups[] = {
    {"share", g_share}, {"delete", g_delete}, {"create", g_create}, {"attrs", g_attrs}, {"times", g_times},
    {"links", g_links}, {"rename", g_rename}, {"io", g_io}, {"find", g_find}, {NULL, NULL},
};

int wmain(int argc, WCHAR **argv)
{
    return wd_main("fsbasic", groups, argc, argv, NULL);
}
