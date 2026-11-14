/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * windiff, file-system semantics 2: names. Reserved device names, trailing
 * dots and spaces, characters a name may not hold, case, long paths,
 * alternate data streams, path normalisation, temporary names, volumes.
 */
#include "wd.h"

typedef struct { USHORT Length, MaximumLength; WCHAR *Buffer; } wd_ustr;
static LONG (WINAPI *pRtlDosPathNameToNtPathName_U_WithStatus)(const WCHAR *, wd_ustr *, WCHAR **, void *);
static ULONG (WINAPI *pRtlDetermineDosPathNameType_U)(const WCHAR *);
static ULONG (WINAPI *pRtlIsDosDeviceName_U)(const WCHAR *);
static void (WINAPI *pRtlFreeUnicodeString)(wd_ustr *);
static HRESULT (WINAPI *pPathCchCanonicalizeEx)(WCHAR *, SIZE_T, const WCHAR *, ULONG);
static HRESULT (WINAPI *pPathCchCombineEx)(WCHAR *, SIZE_T, const WCHAR *, const WCHAR *, ULONG);
static HRESULT (WINAPI *pPathCchSkipRoot)(const WCHAR *, const WCHAR **);
static HRESULT (WINAPI *pPathCchStripPrefix)(WCHAR *, SIZE_T);
static BOOL (WINAPI *pPathCchIsRoot)(const WCHAR *);
static HRESULT (WINAPI *pPathCchRemoveFileSpec)(WCHAR *, SIZE_T);
static HRESULT (WINAPI *pPathCchRemoveBackslash)(WCHAR *, SIZE_T);
static HRESULT (WINAPI *pPathCchAddExtension)(WCHAR *, SIZE_T, const WCHAR *);
static HRESULT (WINAPI *pPathCchFindExtension)(const WCHAR *, SIZE_T, const WCHAR **);
static BOOL (WINAPI *pPathIsUNCEx)(const WCHAR *, const WCHAR **);

static void load(void)
{
    HMODULE nt = GetModuleHandleW(L"ntdll.dll"), kb = GetModuleHandleW(L"kernelbase.dll");
#define GET(m, f) p##f = (void *)GetProcAddress(m, #f)
    GET(nt, RtlDosPathNameToNtPathName_U_WithStatus); GET(nt, RtlDetermineDosPathNameType_U);
    GET(nt, RtlIsDosDeviceName_U); GET(nt, RtlFreeUnicodeString);
    GET(kb, PathCchCanonicalizeEx); GET(kb, PathCchCombineEx); GET(kb, PathCchSkipRoot); GET(kb, PathCchStripPrefix);
    GET(kb, PathCchIsRoot); GET(kb, PathCchRemoveFileSpec); GET(kb, PathCchRemoveBackslash);
    GET(kb, PathCchAddExtension); GET(kb, PathCchFindExtension); GET(kb, PathIsUNCEx);
#undef GET
}

/* ---- names that are special ------------------------------------------- */

static const struct { const char *id; const WCHAR *name; } names[] = {
    {"nul", L"nul"}, {"NUL-upper", L"NUL"}, {"nul.txt", L"nul.txt"}, {"nul.tar.gz", L"NUL.tar.gz"},
    {"nul-colon", L"nul:"}, {"nul-space", L"nul "}, {"nul-dot", L"nul."}, {"nul-space-ext", L"nul .txt"},
    {"sub-nul", L"sub\\nul"}, {"dot-nul", L".\\nul"}, {"nul-sub", L"nul\\x"},
    {"con", L"con"}, {"con.txt", L"CON.txt"}, {"con-colon", L"con:"}, {"conin", L"CONIN$"},
    {"conout", L"CONOUT$"}, {"conin.txt", L"conin$.txt"}, {"aux", L"aux"}, {"aux.c", L"aux.c"}, {"prn", L"prn"},
    {"prn.txt", L"prn.txt"}, {"com1", L"com1"}, {"com1.txt", L"COM1.txt"}, {"com9", L"com9"}, {"com0", L"com0"},
    {"com10", L"com10"}, {"com-superscript-1", L"COM\x00b9"}, {"com-superscript-2", L"com\x00b2.txt"},
    {"lpt1", L"lpt1"}, {"lpt9.x", L"LPT9.x"}, {"lpt0", L"lpt0"}, {"lpt-superscript-3", L"LPT\x00b3"},
    {"clock", L"clock$"}, {"config", L"config$"}, {"xnul", L"xnul"}, {"nulx", L"nulx"}, {"nul1", L"nul1"},
    {"trailing-dot", L"a."}, {"trailing-dots", L"a..."}, {"trailing-space", L"a "}, {"trailing-spaces", L"a   "},
    {"trailing-dot-space", L"a. "}, {"trailing-space-dot", L"a ."}, {"trailing-mixed", L"a. . ."},
    {"leading-space", L" a"}, {"leading-dot", L".a"}, {"leading-dots", L"..a"}, {"three-dots", L"..."},
    {"dot-space-dot", L". ."}, {"space-only", L" "}, {"spaces-only", L"   "}, {"inner-dot-trailing", L"a.b."},
    {"ext-trailing-space", L"a.txt "}, {"dir-trailing-dot", L"sub.\\x"}, {"dir-trailing-space", L"sub \\x"},
    {"lt", L"a<b"}, {"gt", L"a>b"}, {"pipe", L"a|b"}, {"quote", L"a\"b"}, {"question", L"a?b"}, {"star", L"a*b"},
    {"colon", L"a:b"}, {"colon-end", L"a:"}, {"colon-start", L":a"}, {"tab", L"a\tb"}, {"ctrl-1", L"a\x01" L"b"},
    {"ctrl-1f", L"a\x1f" L"b"}, {"del-7f", L"a\x7f" L"b"}, {"slash", L"a/b"}, {"double-backslash", L"a\\\\b"},
    {"unicode", L"\x00fc\x4e2d\x6587.txt"}, {"lone-surrogate", L"a\xd800" L"b"}, {"bom", L"a\xfeff" L"b"},
    {"nbsp-end", L"a\x00a0"}, {"ideographic-space-end", L"a\x3000"}, {"fullwidth-dot-end", L"a\xff0e"},
    {"percent", L"100%"}, {"tilde", L"~"}, {"caret-amp", L"^&;,=+[]{}'`!@#$()"}, {"dollar-data", L"a::$DATA"},
};

static void g_names(void)
{
    unsigned i;
    char id[96];
    HANDLE h;
    WCHAR full[700], *file;
    DWORD n;

    for (i = 0; i < ARRAY_SIZE(names); i++)
    {
        wd_clean();
        CreateDirectoryW(L"sub", NULL);
        _snprintf(id, sizeof(id), "%s.attr-before", names[i].id);
        wd_emit(id, "%s", wd_attr(names[i].name));
        _snprintf(id, sizeof(id), "%s.create", names[i].id);
        h = wd_open(names[i].name, GENERIC_READ | GENERIC_WRITE, WD_RWD, CREATE_NEW, 0);
        if (h == INVALID_HANDLE_VALUE) wd_emit(id, "e=%lu", GetLastError());
        else
        {
            n = GetFileType(h);
            CloseHandle(h);
            wd_emit(id, "ok type=%lu ls=%s sub=%s", n, wd_ls(L"."), wd_ls(L"sub"));
        }
        wd_clean();
        CreateDirectoryW(L"sub", NULL);
        _snprintf(id, sizeof(id), "%s.mkdir", names[i].id);
        if (CreateDirectoryW(names[i].name, NULL)) wd_emit(id, "ok ls=%s sub=%s", wd_ls(L"."), wd_ls(L"sub"));
        else wd_emit(id, "e=%lu", GetLastError());
        _snprintf(id, sizeof(id), "%s.full", names[i].id);
        full[0] = 0; file = NULL;
        n = GetFullPathNameW(names[i].name, 690, full, &file);
        if (!n) wd_emit(id, "e=%lu", GetLastError());
        else wd_emit(id, "%s file=%s", wd_path(full), file ? wd_u8(file) : "(null)");
    }
}

/* ---- case ------------------------------------------------------------- */

static void g_case(void)
{
    static const struct { const char *id; const WCHAR *a, *b; } pairs[] = {
        {"ascii", L"MixedCase.TXT", L"mixedcase.txt"}, {"latin1-umlaut", L"\x00c4" L"B.txt", L"\x00e4" L"b.txt"},
        {"y-diaeresis", L"\x00ff.txt", L"\x0178.txt"}, {"sharp-s", L"stra\x00df" L"e", L"STRASSE"},
        {"capital-sharp-s", L"\x00df", L"\x1e9e"}, {"dotless-i", L"\x0131.txt", L"I.txt"},
        {"dotted-capital-i", L"\x0130.txt", L"i.txt"}, {"long-s", L"\x017f.txt", L"S.txt"},
        {"micro-mu", L"\x00b5.txt", L"\x039c.txt"}, {"final-sigma", L"\x03c2.txt", L"\x03a3.txt"},
        {"sigma", L"\x03c3.txt", L"\x03a3.txt"}, {"cyrillic", L"\x0436.txt", L"\x0416.txt"},
        {"dz-digraph", L"\x01c6.txt", L"\x01c4.txt"}, {"dz-titlecase", L"\x01c5.txt", L"\x01c4.txt"},
        {"fullwidth", L"\xff41.txt", L"\xff21.txt"}, {"georgian", L"\x10d0.txt", L"\x1c90.txt"},
        {"cherokee", L"\xab70.txt", L"\x13a0.txt"}, {"armenian", L"\x0561.txt", L"\x0531.txt"},
        {"roman-numeral", L"\x2170.txt", L"\x2160.txt"}, {"circled", L"\x24d0.txt", L"\x24b6.txt"},
        {"kelvin", L"\x212a.txt", L"k.txt"}, {"angstrom", L"\x212b.txt", L"\x00e5.txt"},
        {"ohm", L"\x2126.txt", L"\x03c9.txt"}, {"deseret", L"\xd801\xdc28.txt", L"\xd801\xdc00.txt"},
        {"precomposed-vs-combining", L"\x00e9.txt", L"e\x0301.txt"}, {"glagolitic", L"\x2c30.txt", L"\x2c00.txt"},
        {"latin-ext-b", L"\x0180.txt", L"\x0243.txt"}, {"n-with-long-leg", L"\x019e.txt", L"\x0220.txt"},
    };
    unsigned i;
    char id[80];
    HANDLE h;
    WCHAR buf[300];
    DWORD n;
    WIN32_FIND_DATAW fd;

    for (i = 0; i < ARRAY_SIZE(pairs); i++)
    {
        wd_clean();
        wd_mkfile(pairs[i].a, "x");
        _snprintf(id, sizeof(id), "pair.%s", pairs[i].id);
        wd_emit(id, "%s", wd_attr(pairs[i].b));
    }
    wd_clean();
    wd_mkfile(L"MixedCase.TXT", "x");
    h = wd_open(L"MIXEDCASE.txt", GENERIC_WRITE, WD_RWD, CREATE_NEW, 0);
    EMIT_H("create-new-other-case", h); CLOSE(h);
    h = FindFirstFileW(L"mixedcase.txt", &fd);
    wd_emit("find-returns-stored-case", "%s", h != INVALID_HANDLE_VALUE ? wd_u8(fd.cFileName) : "not found");
    if (h != INVALID_HANDLE_VALUE) FindClose(h);
    buf[0] = 0; n = GetFullPathNameW(L"mixedcase.txt", 290, buf, NULL);
    wd_emit("GetFullPathName-keeps-given-case", "%s", wd_path(buf));
    EMIT_B("rename-case-only", MoveFileW(L"MixedCase.TXT", L"mixedcase.txt"));
    wd_emit("listing-after-case-rename", "%s", wd_ls(L"."));
    EMIT_B("rename-to-identical-name", MoveFileW(L"mixedcase.txt", L"mixedcase.txt"));
    h = wd_open(L"MIXEDCASE.TXT", GENERIC_WRITE, WD_RWD, CREATE_ALWAYS, 0);
    CLOSE(h);
    wd_emit("create-always-keeps-old-case", "%s", wd_ls(L"."));
    EMIT_B("mkdir-other-case-of-file", CreateDirectoryW(L"MIXEDCASE.TXT", NULL));
    CreateDirectoryW(L"Dir", NULL);
    EMIT_B("mkdir-other-case-of-dir", CreateDirectoryW(L"DIR", NULL));
    wd_mkfile(L"dir\\File", "x");
    wd_emit("path-through-other-case-dir", "%s", wd_attr(L"DIR\\FILE"));
    EMIT_B("SetCurrentDirectory-other-case", SetCurrentDirectoryW(L"DIR"));
    buf[0] = 0; GetCurrentDirectoryW(290, buf);
    wd_emit("GetCurrentDirectory-after-other-case", "%s", wd_path(buf));
    SetCurrentDirectoryW(wd_cwd);
    EMIT_B("CopyFile-onto-other-case-of-itself", CopyFileW(L"dir\\File", L"DIR\\FILE", FALSE));
    EMIT_B("hardlink-other-case", CreateHardLinkW(L"dir\\FILE", L"dir\\File", NULL));
}

/* ---- long paths ------------------------------------------------------- */

static void fill(WCHAR *p, int n, WCHAR c) { int i; for (i = 0; i < n; i++) p[i] = c; p[n] = 0; }

static void g_longpath(void)
{
    static WCHAR base[1200], path[1600], rel[600], buf[1600];
    static const int totals[] = {240, 246, 247, 248, 249, 258, 259, 260, 261, 300};
    static const int comps[] = {200, 254, 255, 256, 300};
    unsigned i;
    int cl = lstrlenW(wd_cwd), n;
    char id[80];
    HANDLE h;
    DWORD r;

    /* a file whose whole path has a given length, relative and absolute;
     * the path is the scratch directory, a 100-character directory and a
     * name that makes up the rest */
    for (i = 0; i < ARRAY_SIZE(totals); i++)
    {
        fill(rel, 100, 'd');
        CreateDirectoryW(rel, NULL);
        n = totals[i] - cl - 102;
        rel[100] = '\\';
        fill(rel + 101, n, 'f');
        _snwprintf(path, 1500, L"%ls\\%ls", wd_cwd, rel);
        _snprintf(id, sizeof(id), "file.absolute-%d", totals[i]);
        h = wd_open(path, GENERIC_WRITE, WD_RWD, CREATE_ALWAYS, 0);
        EMIT_H(id, h); CLOSE(h);
        _snprintf(id, sizeof(id), "file.relative-%d", totals[i]);
        h = wd_open(rel, GENERIC_WRITE, WD_RWD, CREATE_ALWAYS, 0);
        EMIT_H(id, h); CLOSE(h);
        _snprintf(id, sizeof(id), "attr.absolute-%d", totals[i]);
        wd_emit(id, "%s", wd_attr(path));
        _snprintf(id, sizeof(id), "delete.absolute-%d", totals[i]);
        EMIT_B(id, DeleteFileW(path));
        _snprintf(id, sizeof(id), "mkdir.absolute-%d", totals[i]);
        EMIT_B(id, CreateDirectoryW(path, NULL));
        _snprintf(id, sizeof(id), "chdir.absolute-%d", totals[i]);
        EMIT_B(id, SetCurrentDirectoryW(path));
        SetCurrentDirectoryW(wd_cwd);
        wd_clean();
    }
    for (i = 0; i < ARRAY_SIZE(comps); i++)
    {
        fill(rel, comps[i], 'c');
        _snwprintf(path, 1500, L"\\\\?\\%ls\\%ls", wd_cwd, rel);
        _snprintf(id, sizeof(id), "component-%d.prefixed-create", comps[i]);
        h = wd_open(path, GENERIC_WRITE, WD_RWD, CREATE_ALWAYS, 0);
        EMIT_H(id, h); CLOSE(h);
        _snprintf(id, sizeof(id), "component-%d.prefixed-mkdir", comps[i]);
        lstrcatW(path, L"d");
        EMIT_B(id, CreateDirectoryW(path, NULL));
        wd_clean();
    }

    /* a deep tree made with the \\?\ prefix */
    _snwprintf(base, 1100, L"\\\\?\\%ls", wd_cwd);
    for (i = 0; i < 4; i++)
    {
        n = lstrlenW(base);
        base[n] = '\\';
        fill(base + n + 1, 100, (WCHAR)('p' + i));
        EMIT_B(i == 0 ? "deep.mkdir-1" : i == 1 ? "deep.mkdir-2" : i == 2 ? "deep.mkdir-3" : "deep.mkdir-4",
               CreateDirectoryW(base, NULL));
    }
    _snwprintf(path, 1500, L"%ls\\file.txt", base);
    h = wd_open(path, GENERIC_WRITE, WD_RWD, CREATE_NEW, 0);
    EMIT_H("deep.create-prefixed", h); CLOSE(h);
    wd_emit("deep.attr-prefixed", "%s", wd_attr(path));
    wd_emit("deep.attr-not-prefixed", "%s", wd_attr(path + 4));
    h = wd_open(path + 4, GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("deep.open-not-prefixed", h); CLOSE(h);
    EMIT_B("deep.chdir-prefixed", SetCurrentDirectoryW(base));
    r = GetCurrentDirectoryW(1500, buf);
    wd_emit("deep.GetCurrentDirectory-length", "%lu", r);
    SetCurrentDirectoryW(wd_cwd);
    r = GetFullPathNameW(path + 4, 1500, buf, NULL);
    wd_emit("deep.GetFullPathName-length-minus-input", "%ld", (long)r - (long)lstrlenW(path + 4));
    r = GetLongPathNameW(path, buf, 1500);
    wd_emit("deep.GetLongPathName-prefixed", "%s", wd_res(r != 0));
    r = GetLongPathNameW(path + 4, buf, 1500);
    wd_emit("deep.GetLongPathName-not-prefixed", "%s", wd_res(r != 0));
    r = GetShortPathNameW(path, buf, 1500);
    wd_emit("deep.GetShortPathName-prefixed", "%s", wd_res(r != 0));
    {
        WIN32_FIND_DATAW fd;
        _snwprintf(buf, 1500, L"%ls\\*", base);
        h = FindFirstFileW(buf, &fd);
        EMIT_H("deep.find-prefixed", h);
        if (h != INVALID_HANDLE_VALUE) FindClose(h);
        h = FindFirstFileW(buf + 4, &fd);
        EMIT_H("deep.find-not-prefixed", h);
        if (h != INVALID_HANDLE_VALUE) FindClose(h);
    }
    _snwprintf(buf, 1500, L"%ls\\file2.txt", base);
    EMIT_B("deep.MoveFile-prefixed", MoveFileW(path, buf));
    EMIT_B("deep.CopyFile-prefixed", CopyFileW(buf, path, TRUE));
    EMIT_B("deep.CopyFile-not-prefixed-target", CopyFileW(buf, path + 4, FALSE));
    EMIT_B("deep.DeleteFile-not-prefixed", DeleteFileW(path + 4));
    EMIT_B("deep.DeleteFile-prefixed", DeleteFileW(path));
    EMIT_B("deep.RemoveDirectory-not-prefixed", RemoveDirectoryW(base + 4));

    /* what the prefix switches off */
    wd_mkfile(L"plain", "x");
    CreateDirectoryW(L"sub", NULL);
    _snwprintf(path, 1500, L"\\\\?\\%ls\\sub\\..\\plain", wd_cwd);
    wd_emit("prefix.dotdot", "%s", wd_attr(path));
    _snwprintf(path, 1500, L"\\\\?\\%ls\\.\\plain", wd_cwd);
    wd_emit("prefix.dot", "%s", wd_attr(path));
    _snwprintf(path, 1500, L"\\\\?\\%ls/plain", wd_cwd);
    wd_emit("prefix.forward-slash", "%s", wd_attr(path));
    _snwprintf(path, 1500, L"\\\\?\\%ls\\\\plain", wd_cwd);
    wd_emit("prefix.double-backslash", "%s", wd_attr(path));
    _snwprintf(path, 1500, L"\\\\?\\%ls\\plain.", wd_cwd);
    wd_emit("prefix.trailing-dot", "%s", wd_attr(path));
    _snwprintf(path, 1500, L"\\\\?\\%ls\\plain ", wd_cwd);
    wd_emit("prefix.trailing-space", "%s", wd_attr(path));
    _snwprintf(path, 1500, L"\\\\?\\%ls\\PLAIN", wd_cwd);
    wd_emit("prefix.other-case", "%s", wd_attr(path));
    wd_emit("prefix.relative", "%s", wd_attr(L"\\\\?\\plain"));
    _snwprintf(path, 1500, L"\\\\.\\%ls\\sub\\..\\plain", wd_cwd);
    wd_emit("device-prefix.dotdot", "%s", wd_attr(path));
    _snwprintf(path, 1500, L"\\??\\%ls\\plain", wd_cwd);
    wd_emit("nt-prefix", "%s", wd_attr(path));
    _snwprintf(path, 1500, L"//?/%ls/plain", wd_cwd);
    wd_emit("prefix.forward-slashes-throughout", "%s", wd_attr(path));
    _snwprintf(path, 1500, L"\\\\?\\%ls\\nul", wd_cwd);
    h = wd_open(path, GENERIC_WRITE, WD_RWD, CREATE_NEW, 0);
    EMIT_H("prefix.create-nul", h); CLOSE(h);
    wd_emit("prefix.listing-after-create-nul", "%s", wd_ls(L"."));
    _snwprintf(path, 1500, L"\\\\?\\%ls\\dot.", wd_cwd);
    h = wd_open(path, GENERIC_WRITE, WD_RWD, CREATE_NEW, 0);
    EMIT_H("prefix.create-trailing-dot", h); CLOSE(h);
    wd_emit("prefix.listing-after-create-trailing-dot", "%s", wd_ls(L"."));
    wd_emit("prefix.trailing-dot-name-without-prefix", "%s", wd_attr(L"dot."));
    EMIT_B("prefix.DeleteFile-trailing-dot-without-prefix", DeleteFileW(L"dot."));
}

/* ---- alternate data streams ------------------------------------------- */

static void g_streams(void)
{
    HANDLE h;
    char buf[32];
    DWORD n;
    BOOL ok;
    WCHAR full[400];
    WIN32_FIND_STREAM_DATA sd;
    char list[600];
    int len = 0;

    wd_mkfile(L"s.txt", "main");
    h = wd_open(L"s.txt:alt", GENERIC_READ | GENERIC_WRITE, WD_RWD, CREATE_NEW, 0);
    EMIT_H("create-stream", h);
    ok = wd_ok(WriteFile(h, "stream-data", 11, &n, NULL));
    wd_emit("write-stream", "%s", wd_was());
    CLOSE(h);
    h = wd_open(L"s.txt", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    wd_emit("main-size-after-stream-write", "%lu", GetFileSize(h, NULL));
    CLOSE(h);
    h = wd_open(L"s.txt:alt", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    memset(buf, 0, sizeof(buf)); n = 0;
    ok = wd_ok(ReadFile(h, buf, 30, &n, NULL));
    wd_emit("read-stream", "%s n=%lu %s", wd_was(), n, buf);
    CLOSE(h);
    h = wd_open(L"s.txt::$DATA", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("open-default-stream-by-name", h);
    memset(buf, 0, sizeof(buf)); n = 0;
    ReadFile(h, buf, 30, &n, NULL);
    wd_emit("read-default-stream-by-name", "n=%lu %s", n, buf);
    CLOSE(h);
    h = wd_open(L"s.txt:alt:$DATA", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("open-stream-with-type", h); CLOSE(h);
    h = wd_open(L"s.txt:ALT", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("open-stream-other-case", h); CLOSE(h);
    h = wd_open(L"s.txt:alt:$INDEX_ALLOCATION", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("open-stream-wrong-type", h); CLOSE(h);
    h = wd_open(L"s.txt:nostream", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("open-missing-stream", h); CLOSE(h);
    h = wd_open(L"nofile:alt", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("open-stream-of-missing-file", h); CLOSE(h);
    h = wd_open(L"new.txt:alt", GENERIC_WRITE, WD_RWD, CREATE_NEW, 0);
    EMIT_H("create-stream-creates-file", h); CLOSE(h);
    wd_emit("attr-of-file-created-by-stream", "%s", wd_attr(L"new.txt"));
    wd_emit("attr-of-stream-name", "%s", wd_attr(L"s.txt:alt"));
    wd_emit("listing", "%s", wd_ls(L"."));
    h = FindFirstStreamW(L"s.txt", FindStreamInfoStandard, &sd, 0);
    if (h == INVALID_HANDLE_VALUE) wd_emit("FindFirstStream", "e=%lu", GetLastError());
    else
    {
        do len += _snprintf(list + len, 120, "%s(%lu) ", wd_u8(sd.cStreamName), sd.StreamSize.LowPart);
        while (len < 400 && FindNextStreamW(h, &sd));
        list[len] = 0;
        wd_emit("FindFirstStream", "%s", list);
        FindClose(h);
    }
    h = FindFirstStreamW(L"nofile", FindStreamInfoStandard, &sd, 0);
    EMIT_H("FindFirstStream-missing-file", h);
    if (h != INVALID_HANDLE_VALUE) FindClose(h);
    EMIT_B("CopyFile-with-stream", CopyFileW(L"s.txt", L"copy.txt", FALSE));
    wd_emit("stream-of-copy", "%s", wd_attr(L"copy.txt:alt"));
    EMIT_B("MoveFile-with-stream", MoveFileW(L"copy.txt", L"moved.txt"));
    wd_emit("stream-of-moved", "%s", wd_attr(L"moved.txt:alt"));
    EMIT_B("DeleteFile-stream", DeleteFileW(L"s.txt:alt"));
    wd_emit("attr-after-stream-delete", "%s", wd_attr(L"s.txt"));
    EMIT_B("DeleteFile-default-stream-by-name", DeleteFileW(L"moved.txt::$DATA"));
    wd_emit("attr-after-default-stream-delete", "%s", wd_attr(L"moved.txt"));
    CreateDirectoryW(L"d", NULL);
    h = wd_open(L"d:alt", GENERIC_WRITE, WD_RWD, CREATE_NEW, 0);
    EMIT_H("create-stream-on-directory", h); CLOSE(h);
    h = wd_open(L"d::$INDEX_ALLOCATION", GENERIC_READ, WD_RWD, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS);
    EMIT_H("open-directory-index-allocation", h); CLOSE(h);
    h = wd_open(L"d:$I30:$INDEX_ALLOCATION", GENERIC_READ, WD_RWD, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS);
    EMIT_H("open-directory-i30", h); CLOSE(h);
    EMIT_B("mkdir-with-index-allocation-suffix", CreateDirectoryW(L"d2::$INDEX_ALLOCATION", NULL));
    wd_emit("listing-after-mkdir-suffix", "%s", wd_ls(L"."));
    h = wd_open(L":alt", GENERIC_WRITE, WD_RWD, CREATE_NEW, 0);
    EMIT_H("create-stream-with-empty-file-name", h); CLOSE(h);
    h = wd_open(L"s.txt:", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    EMIT_H("open-trailing-colon", h); CLOSE(h);
    h = wd_open(L"s.txt:a:b:c", GENERIC_WRITE, WD_RWD, CREATE_NEW, 0);
    EMIT_H("create-too-many-colons", h); CLOSE(h);
    h = wd_open(L"s.txt:a\\b", GENERIC_WRITE, WD_RWD, CREATE_NEW, 0);
    EMIT_H("create-stream-name-with-backslash", h); CLOSE(h);
    full[0] = 0; n = GetFullPathNameW(L"s.txt:alt", 390, full, NULL);
    wd_emit("GetFullPathName-stream", "%s", n ? wd_path(full) : "failed");
    h = wd_open(L"s.txt", GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    {
        struct { DWORD next, namelen; LARGE_INTEGER size, alloc; WCHAR name[200]; } si;
        memset(&si, 0, sizeof(si));
        ok = wd_ok(GetFileInformationByHandleEx(h, FileStreamInfo, &si, sizeof(si)));
        wd_emit("FileStreamInfo-plain-file", ok ? "ok next=%lu %s size=%lu" : "e=%lu", ok ? si.next : wd_last,
                ok ? wd_u8n(si.name, si.namelen / 2) : "", si.size.LowPart);
    }
    CLOSE(h);
}

/* ---- path normalisation ----------------------------------------------- */

static const struct { const char *id; const WCHAR *p; } paths[] = {
    {"empty", L""}, {"dot", L"."}, {"dotdot", L".."}, {"name", L"foo"}, {"rel", L"foo\\bar"}, {"rel-slash", L"foo/bar"},
    {"rel-dot", L".\\foo"}, {"rel-dotdot", L"..\\foo"}, {"rel-mid-dotdot", L"a\\b\\..\\c"}, {"rel-end-dotdot", L"a\\b\\.."},
    {"rel-end-dot", L"a\\b\\."}, {"rel-end-backslash", L"a\\b\\"}, {"rel-many-backslashes", L"a\\\\\\b"},
    {"three-dots", L"..."}, {"three-dots-mid", L"a\\...\\b"}, {"dots-in-names", L"a...\\b..."},
    {"spaces", L" a \\ b "}, {"trailing-space", L"a\\b   "}, {"trailing-dots", L"a\\b..."}, {"space-dots", L"a\\ ..."},
    {"dot-space-dot", L"a\\. ."}, {"component-space", L"a \\b. \\c."}, {"only-space", L" "},
    {"root", L"\\"}, {"root-name", L"\\foo"}, {"root-slash", L"/foo/bar"}, {"root-dotdot", L"\\..\\..\\foo"},
    {"drive", L"C:"}, {"drive-dot", L"C:."}, {"drive-dotdot", L"C:.."}, {"drive-rel", L"C:foo"},
    {"drive-rel-dotdot", L"C:..\\foo"}, {"drive-root", L"C:\\"}, {"drive-abs", L"C:\\foo\\bar"},
    {"drive-abs-slash", L"C:/foo/./bar/../baz"}, {"drive-above-root", L"C:\\a\\..\\..\\..\\b"},
    {"drive-lower", L"c:\\foo"}, {"other-drive-rel", L"Q:foo"}, {"other-drive-abs", L"Q:\\foo\\..\\bar"},
    {"digit-drive", L"1:\\foo"}, {"colon-drive", L"::\\foo"}, {"two-letter-drive", L"CD:\\foo"},
    {"abs-trailing-backslashes", L"C:\\foo\\\\\\"}, {"abs-dot-dot-backslash", L"C:\\foo\\.\\.\\"},
    {"abs-space-component", L"C:\\foo\\ \\bar"}, {"abs-wildcards", L"C:\\f*?\\b<>"},
    {"nul", L"nul"}, {"nul-abs", L"C:\\foo\\nul"}, {"nul-ext-abs", L"C:\\foo\\nul.txt"}, {"nul-dir", L"C:\\nul\\foo"},
    {"nul-colon", L"nul:"}, {"nul-colon-suffix", L"nul:foo"}, {"nul-space", L"nul  "}, {"con", L"con"},
    {"con-colon-abs", L"C:\\con:"}, {"com1-colon", L"COM1:"}, {"com1-rel", L"foo\\com1"}, {"aux-ext", L"aux.h"},
    {"lpt1-space-ext", L"lpt1 .txt"}, {"conin", L"CONIN$"}, {"conout-abs", L"C:\\foo\\CONOUT$"},
    {"device-nul", L"\\\\.\\nul"}, {"device-pipe", L"\\\\.\\pipe\\foo\\..\\bar"}, {"device-drive", L"\\\\.\\C:\\foo\\..\\bar"},
    {"device-only", L"\\\\."}, {"device-backslash", L"\\\\.\\"}, {"device-slashes", L"//./C:/foo"},
    {"verbatim", L"\\\\?\\C:\\foo\\..\\bar"}, {"verbatim-only", L"\\\\?\\"}, {"verbatim-short", L"\\\\?"},
    {"verbatim-unc", L"\\\\?\\UNC\\server\\share\\..\\x"}, {"verbatim-slash", L"\\\\?\\C:/foo/bar"},
    {"verbatim-rel", L"\\\\?\\foo"}, {"nt-prefix", L"\\??\\C:\\foo\\..\\bar"}, {"mixed-prefix", L"\\/?\\C:\\foo"},
    {"unc", L"\\\\server\\share\\foo\\..\\bar"}, {"unc-root", L"\\\\server\\share"}, {"unc-root-backslash", L"\\\\server\\share\\"},
    {"unc-above-root", L"\\\\server\\share\\..\\..\\x"}, {"unc-server", L"\\\\server"}, {"unc-server-backslash", L"\\\\server\\"},
    {"unc-empty", L"\\\\"}, {"unc-three", L"\\\\\\server\\share"}, {"unc-slashes", L"//server/share/foo"},
    {"unc-dot-server", L"\\\\.server\\share"}, {"url", L"file:///C:/foo"}, {"stream", L"C:\\foo\\a:b:c"},
    {"stream-rel", L"a:b"}, {"tab", L"a\tb"}, {"long-component", L"C:\\foo\\aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\\b"},
};

static void g_fullpath(void)
{
    unsigned i;
    char id[96];
    WCHAR buf[1200], *file;
    DWORD n;

    load();
    for (i = 0; i < ARRAY_SIZE(paths); i++)
    {
        _snprintf(id, sizeof(id), "GetFullPathName.%s", paths[i].id);
        buf[0] = 0; file = NULL;
        SetLastError(0xdeadbeef);
        n = GetFullPathNameW(paths[i].p, 1100, buf, &file);
        if (!n) wd_emit(id, "e=%lu", GetLastError());
        else wd_emit(id, "%s file=%s", wd_path(buf), file ? wd_u8(file) : "(null)");
    }
    n = GetFullPathNameW(L"foo", 0, NULL, NULL);
    wd_emit("GetFullPathName.size-query-minus-cwd", "%ld", (long)n - (long)lstrlenW(wd_cwd));
    buf[0] = 'X'; buf[1] = 0;
    n = GetFullPathNameW(L"foo", 3, buf, NULL);
    wd_emit("GetFullPathName.small-buffer-minus-cwd", "%ld buf0=%c", (long)n - (long)lstrlenW(wd_cwd), (char)buf[0]);
    n = GetFullPathNameW(NULL, 100, buf, NULL);
    wd_emit("GetFullPathName.null", "%lu %s", n, wd_res(n != 0));
}

static void g_ntpath(void)
{
    unsigned i;
    char id[96];
    wd_ustr us;
    WCHAR *file;
    LONG st;

    load();
    for (i = 0; i < ARRAY_SIZE(paths); i++)
    {
        _snprintf(id, sizeof(id), "RtlDosPathNameToNtPathName.%s", paths[i].id);
        if (!pRtlDosPathNameToNtPathName_U_WithStatus) wd_emit(id, "missing-export");
        else
        {
            memset(&us, 0, sizeof(us)); file = NULL;
            st = pRtlDosPathNameToNtPathName_U_WithStatus(paths[i].p, &us, &file, NULL);
            if (st) wd_emit(id, "status=%08lx", (unsigned long)st);
            else
            {
                wd_emit(id, "%s file=%s", wd_path(us.Buffer), file ? wd_u8(file) : "(null)");
                pRtlFreeUnicodeString(&us);
            }
        }
        _snprintf(id, sizeof(id), "RtlDetermineDosPathNameType.%s", paths[i].id);
        if (!pRtlDetermineDosPathNameType_U) wd_emit(id, "missing-export");
        else wd_emit(id, "%lu", pRtlDetermineDosPathNameType_U(paths[i].p));
        _snprintf(id, sizeof(id), "RtlIsDosDeviceName.%s", paths[i].id);
        if (!pRtlIsDosDeviceName_U) wd_emit(id, "missing-export");
        else wd_emit(id, "%08lx", pRtlIsDosDeviceName_U(paths[i].p));
    }
}

static void g_pathcch(void)
{
    unsigned i;
    char id[96];
    WCHAR buf[1200];
    const WCHAR *end;
    HRESULT hr;

    load();
    for (i = 0; i < ARRAY_SIZE(paths); i++)
    {
        _snprintf(id, sizeof(id), "PathCchCanonicalizeEx.%s", paths[i].id);
        if (!pPathCchCanonicalizeEx) wd_emit(id, "missing-export");
        else
        {
            buf[0] = 0;
            hr = pPathCchCanonicalizeEx(buf, 1100, paths[i].p, 0);
            wd_emit(id, "hr=%08lx %s", (unsigned long)hr, wd_u8(buf));
        }
        _snprintf(id, sizeof(id), "PathCchCanonicalizeEx-long.%s", paths[i].id);
        if (pPathCchCanonicalizeEx)
        {
            buf[0] = 0;
            hr = pPathCchCanonicalizeEx(buf, 1100, paths[i].p, 1 /* PATHCCH_ALLOW_LONG_PATHS */);
            wd_emit(id, "hr=%08lx %s", (unsigned long)hr, wd_u8(buf));
        }
        _snprintf(id, sizeof(id), "PathCchSkipRoot.%s", paths[i].id);
        if (!pPathCchSkipRoot) wd_emit(id, "missing-export");
        else
        {
            end = NULL;
            hr = pPathCchSkipRoot(paths[i].p, &end);
            wd_emit(id, "hr=%08lx skip=%d", (unsigned long)hr, end ? (int)(end - paths[i].p) : -1);
        }
        _snprintf(id, sizeof(id), "PathCchIsRoot.%s", paths[i].id);
        if (!pPathCchIsRoot) wd_emit(id, "missing-export");
        else wd_emit(id, "%d", pPathCchIsRoot(paths[i].p));
        _snprintf(id, sizeof(id), "PathCchRemoveFileSpec.%s", paths[i].id);
        if (!pPathCchRemoveFileSpec) wd_emit(id, "missing-export");
        else
        {
            lstrcpynW(buf, paths[i].p, 1100);
            hr = pPathCchRemoveFileSpec(buf, 1100);
            wd_emit(id, "hr=%08lx %s", (unsigned long)hr, wd_u8(buf));
        }
        _snprintf(id, sizeof(id), "PathCchStripPrefix.%s", paths[i].id);
        if (!pPathCchStripPrefix) wd_emit(id, "missing-export");
        else
        {
            lstrcpynW(buf, paths[i].p, 1100);
            hr = pPathCchStripPrefix(buf, 1100);
            wd_emit(id, "hr=%08lx %s", (unsigned long)hr, wd_u8(buf));
        }
        _snprintf(id, sizeof(id), "PathCchCombineEx-onto-C-base.%s", paths[i].id);
        if (!pPathCchCombineEx) wd_emit(id, "missing-export");
        else
        {
            buf[0] = 0;
            hr = pPathCchCombineEx(buf, 1100, L"C:\\base\\dir", paths[i].p, 0);
            wd_emit(id, "hr=%08lx %s", (unsigned long)hr, wd_u8(buf));
        }
        _snprintf(id, sizeof(id), "PathCchRemoveBackslash.%s", paths[i].id);
        if (pPathCchRemoveBackslash)
        {
            lstrcpynW(buf, paths[i].p, 1100);
            hr = pPathCchRemoveBackslash(buf, 1100);
            wd_emit(id, "hr=%08lx %s", (unsigned long)hr, wd_u8(buf));
        }
        _snprintf(id, sizeof(id), "PathCchFindExtension.%s", paths[i].id);
        if (pPathCchFindExtension)
        {
            end = NULL;
            hr = pPathCchFindExtension(paths[i].p, 1100, &end);
            wd_emit(id, "hr=%08lx ext=%s", (unsigned long)hr, end ? wd_u8(end) : "(null)");
        }
        _snprintf(id, sizeof(id), "PathCchAddExtension.%s", paths[i].id);
        if (pPathCchAddExtension)
        {
            lstrcpynW(buf, paths[i].p, 1100);
            hr = pPathCchAddExtension(buf, 1100, L"ext");
            wd_emit(id, "hr=%08lx %s", (unsigned long)hr, wd_u8(buf));
        }
        _snprintf(id, sizeof(id), "PathIsUNCEx.%s", paths[i].id);
        if (pPathIsUNCEx)
        {
            end = NULL;
            wd_emit(id, "%d", pPathIsUNCEx(paths[i].p, &end));
        }
    }
}

/* ---- temporary names, volumes, drives --------------------------------- */

static void g_volume(void)
{
    WCHAR buf[600], name[300], fs[64];
    DWORD n, serial, complen, flags, spc, bps, fc, tc, e;
    UINT u;
    BOOL ok;
    static const struct { const char *id; const WCHAR *p; } roots[] = {
        {"c-root", L"C:\\"}, {"c-no-backslash", L"C:"}, {"c-lower", L"c:\\"}, {"c-slash", L"C:/"}, {"c-dir", L"C:\\wd"},
        {"c-dir-backslash", L"C:\\wd\\"}, {"missing-drive", L"Q:\\"}, {"empty", L""}, {"relative", L"foo"},
        {"dot", L"."}, {"backslash", L"\\"}, {"verbatim", L"\\\\?\\C:\\"},
        {"device", L"\\\\.\\C:\\"}, {"missing-dir", L"C:\\nosuchdir\\"}, {"a-drive", L"A:\\"},
    };
    unsigned i;
    char id[80];

    for (i = 0; i < ARRAY_SIZE(roots); i++)
    {
        _snprintf(id, sizeof(id), "GetDriveType.%s", roots[i].id);
        wd_emit(id, "%u", GetDriveTypeW(roots[i].p));
        _snprintf(id, sizeof(id), "GetVolumeInformation.%s", roots[i].id);
        fs[0] = 0;
        ok = wd_ok(GetVolumeInformationW(roots[i].p, name, 290, &serial, &complen, &flags, fs, 60));
        wd_emit(id, ok ? "ok fs=%s maxlen=%lu" : "e=%lu", ok ? wd_u8(fs) : (const char *)(ULONG_PTR)wd_last, complen);
        _snprintf(id, sizeof(id), "GetDiskFreeSpace.%s", roots[i].id);
        EMIT_B(id, GetDiskFreeSpaceW(roots[i].p, &spc, &bps, &fc, &tc));
        _snprintf(id, sizeof(id), "GetVolumePathName.%s", roots[i].id);
        buf[0] = 0;
        ok = wd_ok(GetVolumePathNameW(roots[i].p, buf, 590));
        wd_emit(id, "%s %s", wd_was(), ok ? wd_path(buf) : "");
    }
    wd_emit("GetDriveType.null", "%u", GetDriveTypeW(NULL));
    ok = wd_ok(GetVolumeInformationW(L"C:\\", NULL, 0, NULL, &complen, &flags, NULL, 0));
    wd_emit("volume-flags-C", ok ? "ok case-preserved=%d case-sensitive=%d unicode=%d acls=%d streams=%d hardlinks=%d sparse=%d reparse=%d" : "e=%lu",
            ok ? !!(flags & FILE_CASE_PRESERVED_NAMES) : (int)wd_last, !!(flags & FILE_CASE_SENSITIVE_SEARCH),
            !!(flags & FILE_UNICODE_ON_DISK), !!(flags & FILE_PERSISTENT_ACLS), !!(flags & FILE_NAMED_STREAMS),
            !!(flags & FILE_SUPPORTS_HARD_LINKS), !!(flags & FILE_SUPPORTS_SPARSE_FILES), !!(flags & FILE_SUPPORTS_REPARSE_POINTS));
    ok = wd_ok(GetVolumeInformationW(L"C:\\", name, 2, NULL, NULL, NULL, fs, 60));
    wd_emit("GetVolumeInformation.small-name-buffer", "%s", wd_was());
    ok = wd_ok(GetVolumeInformationW(L"C:\\", name, 290, NULL, NULL, NULL, fs, 2));
    wd_emit("GetVolumeInformation.small-fs-buffer", "%s", wd_was());
    buf[0] = 0;
    ok = wd_ok(GetVolumeNameForVolumeMountPointW(L"C:\\", buf, 590));
    wd_emit("GetVolumeNameForVolumeMountPoint", "%s len=%d prefix=%s", wd_was(), lstrlenW(buf), wd_u8n(buf, 11));
    ok = wd_ok(GetVolumeNameForVolumeMountPointW(L"C:", buf, 590));
    wd_emit("GetVolumeNameForVolumeMountPoint-no-backslash", "%s", wd_was());
    n = GetLogicalDrives();
    wd_emit("GetLogicalDrives-has-C", "%d", !!(n & 4));

    /* temporary names */
    u = GetTempFileNameW(L".", L"abcdef", 0, buf); e = GetLastError();
    wd_emit("GetTempFileName.unique-0", u ? "ok prefix=%s len=%d exists=%d" : "e=%lu",
            u ? wd_u8n(buf, 5) : (const char *)(ULONG_PTR)e, lstrlenW(buf), GetFileAttributesW(buf) != INVALID_FILE_ATTRIBUTES);
    u = GetTempFileNameW(L".", L"ab", 0x1234, buf); e = GetLastError();
    wd_emit("GetTempFileName.unique-given", u ? "ok %u %s exists=%d" : "e=%lu", u ? u : (UINT)e, wd_u8(buf),
            GetFileAttributesW(buf) != INVALID_FILE_ATTRIBUTES);
    u = GetTempFileNameW(L".", L"ab", 0x12345, buf); e = GetLastError();
    wd_emit("GetTempFileName.unique-17-bits", u ? "ok %u %s" : "e=%lu", u ? u : (UINT)e, wd_u8(buf));
    u = GetTempFileNameW(L".", L"ab", 0x10000, buf); e = GetLastError();
    wd_emit("GetTempFileName.unique-0x10000", u ? "ok %u %s" : "e=%lu", u ? u : (UINT)e, wd_u8(buf));
    u = GetTempFileNameW(L".", NULL, 7, buf); e = GetLastError();
    wd_emit("GetTempFileName.null-prefix", u ? "ok %u %s" : "e=%lu", u ? u : (UINT)e, wd_u8(buf));
    u = GetTempFileNameW(L".", L"", 7, buf); e = GetLastError();
    wd_emit("GetTempFileName.empty-prefix", u ? "ok %u %s" : "e=%lu", u ? u : (UINT)e, wd_u8(buf));
    u = GetTempFileNameW(L"nodir", L"ab", 0, buf); e = GetLastError();
    wd_emit("GetTempFileName.missing-directory", u ? "ok" : "e=%lu", e);
    u = GetTempFileNameW(L"nodir", L"ab", 5, buf); e = GetLastError();
    wd_emit("GetTempFileName.missing-directory-unique-given", u ? "ok %s" : "e=%lu", u ? wd_u8(buf) : (const char *)(ULONG_PTR)e);
    u = GetTempFileNameW(L"", L"ab", 5, buf); e = GetLastError();
    wd_emit("GetTempFileName.empty-directory", u ? "ok %s" : "e=%lu", u ? wd_u8(buf) : (const char *)(ULONG_PTR)e);
    u = GetTempFileNameW(L".\\", L"a.b", 5, buf); e = GetLastError();
    wd_emit("GetTempFileName.dot-in-prefix", u ? "ok %s" : "e=%lu", u ? wd_u8(buf) : (const char *)(ULONG_PTR)e);
    u = GetTempFileNameW(L"C:\\wd\\..\\wd", L"ab", 5, buf); e = GetLastError();
    wd_emit("GetTempFileName.directory-not-normalised", u ? "ok %s" : "e=%lu", u ? wd_u8(buf) : (const char *)(ULONG_PTR)e);
    n = GetTempPathW(590, buf);
    wd_emit("GetTempPath.ends-with-backslash", "%d", n && buf[n - 1] == '\\');
    n = GetTempPathW(2, buf);
    wd_emit("GetTempPath.small-buffer-returns-more", "%d", n > 2);
    SetEnvironmentVariableW(L"TMP", L"C:\\wd\\nosuchdir");
    n = GetTempPathW(590, buf);
    wd_emit("GetTempPath.TMP-missing-directory", "%s", n ? wd_u8(buf) : "failed");
    SetEnvironmentVariableW(L"TMP", L"relative\\dir");
    n = GetTempPathW(590, buf);
    wd_emit("GetTempPath.TMP-relative", "%s", n ? wd_path(buf) : "failed");
    SetEnvironmentVariableW(L"TMP", NULL);
    SetEnvironmentVariableW(L"TEMP", L"C:\\wd\\fromtemp");
    n = GetTempPathW(590, buf);
    wd_emit("GetTempPath.TEMP-when-no-TMP", "%s", n ? wd_u8(buf) : "failed");
    SetEnvironmentVariableW(L"TEMP", NULL);
    SetEnvironmentVariableW(L"USERPROFILE", L"C:\\wd\\profile");
    n = GetTempPathW(590, buf);
    wd_emit("GetTempPath.USERPROFILE-when-neither", "%s", n ? wd_u8(buf) : "failed");
    SetEnvironmentVariableW(L"USERPROFILE", NULL);
    n = GetTempPathW(590, buf);
    wd_emit("GetTempPath.none-set-is-windows-directory", "%d", n && !_wcsnicmp(buf, L"C:\\Windows", 10));

    /* current directory */
    EMIT_B("SetCurrentDirectory.missing", SetCurrentDirectoryW(L"nodir"));
    wd_mkfile(L"f", "x");
    EMIT_B("SetCurrentDirectory.file", SetCurrentDirectoryW(L"f"));
    EMIT_B("SetCurrentDirectory.empty", SetCurrentDirectoryW(L""));
    EMIT_B("SetCurrentDirectory.drive-only", SetCurrentDirectoryW(L"C:"));
    buf[0] = 0; GetCurrentDirectoryW(590, buf);
    wd_emit("GetCurrentDirectory.after-drive-only", "%s", wd_path(buf));
    CreateDirectoryW(L"Sub", NULL);
    EMIT_B("SetCurrentDirectory.trailing-backslash-slash", SetCurrentDirectoryW(L"sub\\/"));
    buf[0] = 0; GetCurrentDirectoryW(590, buf);
    wd_emit("GetCurrentDirectory.stored-form", "%s", wd_path(buf));
    n = GetEnvironmentVariableW(L"=C:", buf, 590);
    wd_emit("drive-current-directory-variable", "%s", n ? wd_path(buf) : "not set");
    SetCurrentDirectoryW(wd_cwd);
    n = GetCurrentDirectoryW(2, buf);
    wd_emit("GetCurrentDirectory.small-buffer-minus-len", "%ld", (long)n - (long)lstrlenW(wd_cwd));
    n = GetCurrentDirectoryW(0, NULL);
    wd_emit("GetCurrentDirectory.size-query-minus-len", "%ld", (long)n - (long)lstrlenW(wd_cwd));
    n = SearchPathW(NULL, L"f", NULL, 590, buf, NULL);
    wd_emit("SearchPath.current-directory", "%s", n ? wd_path(buf) : "not found");
    n = SearchPathW(NULL, L"kernel32", L".dll", 590, buf, NULL);
    wd_emit("SearchPath.system-dll-with-extension", "%s", n ? wd_u8(buf) : "not found");
    n = SearchPathW(NULL, L"kernel32.dll.", L".dll", 590, buf, NULL);
    wd_emit("SearchPath.trailing-dot", "%s", n ? wd_u8(buf) : "not found");
    n = SearchPathW(L".", L"F", L".txt", 590, buf, NULL);
    wd_emit("SearchPath.given-path-extension-not-needed", "%s", n ? wd_path(buf) : "not found");
    n = SearchPathW(NULL, L"..\\..\\wd", NULL, 590, buf, NULL);
    wd_emit("SearchPath.relative-with-dotdot", "%s", n ? wd_path(buf) : "not found");
    n = SearchPathW(NULL, L"", NULL, 590, buf, NULL);
    wd_emit("SearchPath.empty", "%lu %s", n, wd_res(n != 0));
}

static const struct wd_group_def groups[] = {
    {"names", g_names}, {"case", g_case}, {"longpath", g_longpath}, {"streams", g_streams},
    {"fullpath", g_fullpath}, {"ntpath", g_ntpath}, {"pathcch", g_pathcch}, {"volume", g_volume}, {NULL, NULL},
};

int wmain(int argc, WCHAR **argv)
{
    return wd_main("fsname", groups, argc, argv, NULL);
}
