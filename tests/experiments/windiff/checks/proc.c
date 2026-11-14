/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * windiff, processes: how a command line reaches a program, how
 * CreateProcess finds the program, the environment, exit codes, and what a
 * child is started with. The child is wdchild.exe (see wdchild.c).
 */
#include "wd.h"

static WCHAR child_exe[600], out_file[600];

static void init(void)
{
    WCHAR *p, w[80];
    lstrcpyW(child_exe, wd_self);
    p = wcsrchr(child_exe, '\\');
    lstrcpyW(p ? p + 1 : child_exe, L"wdchild.exe");
    GetTempPathW(500, out_file);
    lstrcatW(out_file, L"wdchild.out");
    DeleteFileW(out_file);
    SetEnvironmentVariableW(L"WD_OUT", out_file);
    SetEnvironmentVariableW(L"WD_EXE", L"proc");
    MultiByteToWideChar(CP_UTF8, 0, wd_group, -1, w, 79);
    SetEnvironmentVariableW(L"WD_GROUP", w);
    SetEnvironmentVariableW(L"WD_CWD", wd_cwd);
}

static void set_child(const char *id, const WCHAR *mode)
{
    WCHAR w[300];
    MultiByteToWideChar(CP_UTF8, 0, id, -1, w, 290);
    SetEnvironmentVariableW(L"WD_ID", w);
    SetEnvironmentVariableW(L"WD_MODE", mode);
}

/* Copies what the child wrote to our output. */
static void forward(void)
{
    static char buf[16384];
    DWORD n;
    HANDLE h = wd_open(out_file, GENERIC_READ, WD_RWD, OPEN_EXISTING, 0);
    if (h == INVALID_HANDLE_VALUE) return;
    while (ReadFile(h, buf, sizeof(buf), &n, NULL) && n) wd_write(buf, (int)n);
    CloseHandle(h);
    DeleteFileW(out_file);
}

static BOOL spawn(const WCHAR *app, const WCHAR *cmdline, DWORD flags, void *env, const WCHAR *dir,
                  STARTUPINFOW *psi, BOOL inherit, DWORD *code)
{
    static WCHAR cmd[40000];
    STARTUPINFOW si;
    PROCESS_INFORMATION pi;
    DWORD err;

    memset(&si, 0, sizeof(si));
    si.cb = sizeof(si);
    if (cmdline) lstrcpynW(cmd, cmdline, 39990);
    *code = 0xffffffff;
    if (!CreateProcessW(app, cmdline ? cmd : NULL, NULL, NULL, inherit, flags, env, dir, psi ? psi : &si, &pi)) return FALSE;
    err = GetLastError();
    if (flags & CREATE_SUSPENDED) ResumeThread(pi.hThread);
    if (WaitForSingleObject(pi.hProcess, 30000) != WAIT_OBJECT_0)
    {
        TerminateProcess(pi.hProcess, 0xfffffffe);
        WaitForSingleObject(pi.hProcess, 5000);
    }
    GetExitCodeProcess(pi.hProcess, code);
    CloseHandle(pi.hThread);
    CloseHandle(pi.hProcess);
    SetLastError(err);
    return TRUE;
}

/* ---- the command line ------------------------------------------------- */

static void g_cmdline(void)
{
    static const struct { const char *id; const WCHAR *tail; } tails[] = {
        {"plain", L"a b c"}, {"quoted-space", L"\"a b\" c"}, {"quote-inside", L"a\"b c\"d"},
        {"backslash-quote", L"a\\\"b c"}, {"quoted-backslash-quote", L"\"a\\\"b\" c"},
        {"two-backslashes-quote", L"a\\\\\"b c\" d"}, {"three-backslashes-quote", L"a\\\\\\\"b c"},
        {"quoted-ends-two-backslashes", L"\"a\\\\\" b"}, {"quoted-ends-three-backslashes", L"\"a\\\\\\\" b\" c"},
        {"backslashes-no-quote", L"a\\\\b c\\"}, {"empty-quotes", L"\"\""}, {"empty-quotes-first", L"\"\" a"},
        {"empty-quotes-middle", L"a \"\" b"}, {"four-quotes", L"\"\"\"\""}, {"three-quotes", L"\"\"\""},
        {"doubled-quote-in-quotes", L"\"a\"\"b\" c"}, {"tripled-quote", L"\"a\"\"\"b c"},
        {"doubled-quote-outside", L"a\"\"b"}, {"quotes-around-quotes", L"\"\"a b\"\""},
        {"three-around", L"\"\"\"a b\"\"\""}, {"single-quotes", L"'a b'"}, {"tab-separator", L"a\tb"},
        {"newline-inside", L"a\nb"}, {"leading-spaces", L"   a"}, {"trailing-spaces", L"a   "},
        {"only-spaces", L"   "}, {"unterminated", L"\"a b"}, {"unterminated-middle", L"a \"b c"},
        {"unc", L"\\\\server\\share\\dir file"}, {"dir-trailing-backslash", L"\"C:\\dir with space\\\" next"},
        {"dir-trailing-two-backslashes", L"\"C:\\dir with space\\\\\" next"},
        {"shell-characters", L"^a %PATH% !x! & | < >"}, {"wildcards", L"*.none ?.none"},
        {"wildcard-matching", L"glob*.txt"}, {"wildcard-quoted", L"\"glob*.txt\""},
        {"adjacent-quoted", L"\"a b\"c\"d e\""}, {"escaped-quotes-bare", L"a\\\"b\\\"c"},
        {"escaped-quoted-arg", L"\\\"a\\\""}, {"quoted-escaped-quotes", L"\"\\\"a b\\\"\""},
        {"lone-backslash", L"a \\ b"}, {"double-backslash-arg", L"a \\\\ b"}, {"only-backslash", L"\\"},
        {"quoted-backslashes", L"\"\\\\\""}, {"quoted-four-backslashes", L"\"a b\\\\\\\\\" c"},
        {"separators", L"a,b;c=d"}, {"response-file", L"@file"}, {"option-quoted-value", L"-opt=\"val ue\""},
        {"switch-quoted", L"/switch:\"a b\""}, {"non-ascii", L"\x00e9\x4e2d \x00fc"}, {"quote-at-end", L"a\""},
        {"backslash-quote-at-end", L"a\\\""}, {"space-quote-space", L"a \" b"}, {"two-empty", L"a \"\" \"\" b"},
        {"vertical-tab", L"a\vb"}, {"form-feed", L"a\fb"}, {"nbsp", L"a\x00a0" L"b"},
    };
    static const struct { const char *id; const WCHAR *cmd; } zero[] = {
        {"argv0-other-name", L"fake name a"}, {"argv0-empty-command-line", L""}, {"argv0-leading-space", L" x y"},
        {"argv0-quote-inside", L"fa\"ke na\"me a"}, {"argv0-backslash-quote", L"\"dir\\\" a b\""},
        {"argv0-tab", L"fake\ta"}, {"argv0-quoted-empty", L"\"\" a"}, {"argv0-only-quote", L"\""},
    };
    static WCHAR cmd[12000], big[6000];
    unsigned i;
    DWORD code;
    char id[100];

    init();
    wd_mkfile(L"glob1.txt", "x"); wd_mkfile(L"glob2.txt", "x");
    for (i = 0; i < ARRAY_SIZE(tails); i++)
    {
        _snwprintf(cmd, 11990, L"\"%ls\" %ls", child_exe, tails[i].tail);
        set_child(tails[i].id, L"argv");
        if (!spawn(NULL, cmd, 0, NULL, NULL, NULL, TRUE, &code))
        {
            _snprintf(id, sizeof(id), "%s.start", tails[i].id);
            wd_emit(id, "e=%lu", GetLastError());
        }
        forward();
    }
    for (i = 0; i < ARRAY_SIZE(zero); i++)
    {
        set_child(zero[i].id, L"argv");
        if (!spawn(child_exe, zero[i].cmd, 0, NULL, NULL, NULL, TRUE, &code))
        {
            _snprintf(id, sizeof(id), "%s.start", zero[i].id);
            wd_emit(id, "e=%lu", GetLastError());
        }
        forward();
    }
    set_child("argv0-null-command-line", L"argv");
    if (!spawn(child_exe, NULL, 0, NULL, NULL, NULL, TRUE, &code)) wd_emit("argv0-null-command-line.start", "e=%lu", GetLastError());
    forward();
    _snwprintf(cmd, 11990, L"%ls a", child_exe);
    set_child("argv0-unquoted", L"argv");
    spawn(NULL, cmd, 0, NULL, NULL, NULL, TRUE, &code);
    forward();
    for (i = 0; i < 5000; i++) big[i] = (WCHAR)('a' + i % 26);
    big[5000] = 0;
    _snwprintf(cmd, 11990, L"\"%ls\" %ls end", child_exe, big);
    set_child("long-argument", L"argv");
    spawn(NULL, cmd, 0, NULL, NULL, NULL, TRUE, &code);
    forward();
}

/* ---- how CreateProcess finds the program ------------------------------ */

static void resolve(const char *id, const WCHAR *app, const WCHAR *cmdline, const WCHAR *dir)
{
    DWORD code;
    char cid[100];
    BOOL ok;

    set_child(id, L"which");
    ok = spawn(app, cmdline, 0, NULL, dir, NULL, TRUE, &code);
    _snprintf(cid, sizeof(cid), "%s.start", id);
    if (ok) wd_emit(cid, "started code=%lu", code);
    else wd_emit(cid, "e=%lu", GetLastError());
    forward();
}

static void g_resolve(void)
{
    static WCHAR path[33000], cmd[33000];
    DWORD n;
    UINT u;
    int i;

    init();
    CreateDirectoryW(L"dir with space", NULL); CreateDirectoryW(L"sub", NULL); CreateDirectoryW(L"pathdir", NULL);
    CopyFileW(child_exe, L"prog.exe", FALSE); CopyFileW(child_exe, L"prog", FALSE); CopyFileW(child_exe, L"prog.txt", FALSE);
    CopyFileW(child_exe, L"dir with space\\prog.exe", FALSE); CopyFileW(child_exe, L"sub\\prog.exe", FALSE);
    CopyFileW(child_exe, L"pathdir\\onpath.exe", FALSE); CopyFileW(child_exe, L"wdchild.exe", FALSE);
    CopyFileW(child_exe, L"two.ext.exe", FALSE); CopyFileW(child_exe, L"prog.com", FALSE);
    wd_mkfile(L"notexe.exe", "this is not a program");
    wd_mkfile(L"empty.exe", "");
    wd_mkfile(L"mz.exe", "MZ");
    wd_mkfile(L"notexe.txt", "text");
    wd_mkfile(L"prog.bat", "@exit /b 7\r\n");
    wd_mkfile(L"prog.cmd", "@exit /b 8\r\n");
    wd_mkfile(L"onlybat.bat", "@exit /b 9\r\n");

    resolve("name-without-extension", NULL, L"prog", NULL);
    resolve("name-with-extension", NULL, L"prog.exe", NULL);
    resolve("name-trailing-dot", NULL, L"prog.", NULL);
    resolve("name-and-argument", NULL, L"prog arg", NULL);
    resolve("name-upper-case", NULL, L"PROG.EXE", NULL);
    resolve("name-two-extensions-without-exe", NULL, L"two.ext", NULL);
    resolve("name-com", NULL, L"prog.com", NULL);
    resolve("dot-backslash", NULL, L".\\prog", NULL);
    resolve("forward-slash", NULL, L"sub/prog.exe", NULL);
    resolve("forward-slash-no-extension", NULL, L"sub/prog arg", NULL);
    resolve("unquoted-space-in-directory", NULL, L"dir with space\\prog arg", NULL);
    resolve("quoted-space-in-directory", NULL, L"\"dir with space\\prog\" arg", NULL);
    CopyFileW(child_exe, L"dir with.exe", FALSE);
    resolve("unquoted-space-with-dir-with-exe", NULL, L"dir with space\\prog arg", NULL);
    CopyFileW(child_exe, L"dir.exe", FALSE);
    resolve("unquoted-space-with-dir-exe", NULL, L"dir with space\\prog arg", NULL);
    resolve("quoted-space-with-dir-exe", NULL, L"\"dir with space\\prog\" arg", NULL);
    DeleteFileW(L"dir.exe"); DeleteFileW(L"dir with.exe");
    resolve("unterminated-quote", NULL, L"\"prog.exe", NULL);
    resolve("tab-separator", NULL, L"prog.exe\targ", NULL);
    resolve("leading-space", NULL, L"  prog.exe", NULL);
    resolve("trailing-backslash", NULL, L"prog.exe\\", NULL);
    resolve("missing", NULL, L"nofile arg", NULL);
    resolve("missing-directory", NULL, L"nodir\\prog.exe", NULL);
    resolve("both-null", NULL, NULL, NULL);
    resolve("empty-command-line", NULL, L"", NULL);
    resolve("only-spaces", NULL, L"   ", NULL);
    resolve("text-file-exe", NULL, L"notexe.exe", NULL);
    resolve("empty-file-exe", NULL, L"empty.exe", NULL);
    resolve("mz-only-exe", NULL, L"mz.exe", NULL);
    resolve("program-with-txt-extension", NULL, L"prog.txt", NULL);
    resolve("text-with-txt-extension", NULL, L"notexe.txt", NULL);
    resolve("directory", NULL, L"sub", NULL);
    resolve("bat", NULL, L"prog.bat", NULL);
    resolve("cmd-with-argument", NULL, L"prog.cmd arg", NULL);
    resolve("bat-without-extension", NULL, L"onlybat", NULL);
    resolve("bat-quoted", NULL, L"\"prog.bat\" \"a b\"", NULL);
    resolve("app-with-extension", L"prog.exe", NULL, NULL);
    resolve("app-without-extension", L"prog", NULL, NULL);
    resolve("app-two-extensions-without-exe", L"two.ext", NULL, NULL);
    resolve("app-missing", L"nofile", NULL, NULL);
    resolve("app-txt", L"prog.txt", NULL, NULL);
    resolve("app-text-file", L"notexe.exe", NULL, NULL);
    resolve("app-directory", L"sub", NULL, NULL);
    resolve("app-bat", L"prog.bat", NULL, NULL);
    resolve("app-bat-with-command-line", L"prog.bat", L"prog.bat arg", NULL);
    resolve("app-empty", L"", L"prog.exe", NULL);
    resolve("app-quoted", L"\"prog.exe\"", NULL, NULL);
    resolve("app-trailing-space", L"prog.exe ", NULL, NULL);
    resolve("app-forward-slash", L"sub/prog.exe", NULL, NULL);
    resolve("app-and-unrelated-command-line", L"prog.exe", L"nofile.exe arg", NULL);
    resolve("directory-parameter-missing", NULL, L"prog.exe", L"nodir");
    resolve("directory-parameter-is-file", NULL, L"prog.exe", L"notexe.txt");
    resolve("directory-parameter-empty", NULL, L"prog.exe", L"");
    resolve("directory-parameter-relative-program", NULL, L"prog.exe", L"sub");
    resolve("directory-parameter-app-relative", L"prog.exe", NULL, L"sub");
    resolve("system-directory-cmd", NULL, L"cmd /c exit 5", NULL);
    resolve("system-directory-cmd-exe", NULL, L"cmd.exe /c exit 6", NULL);
    resolve("application-directory-before-current", NULL, L"wdchild", NULL);
    resolve("not-on-path", NULL, L"onpath", NULL);
    n = GetEnvironmentVariableW(L"PATH", path, 32000);
    _snwprintf(cmd, 32900, L"%ls\\pathdir;%ls", wd_cwd, n ? path : L"");
    SetEnvironmentVariableW(L"PATH", cmd);
    resolve("on-path", NULL, L"onpath", NULL);
    resolve("on-path-app-name", L"onpath.exe", NULL, NULL);
    SetEnvironmentVariableW(L"PATH", L"pathdir");
    resolve("on-relative-path", NULL, L"onpath", NULL);
    SetEnvironmentVariableW(L"PATH", NULL);
    resolve("no-path-variable-system-directory", NULL, L"cmd /c exit 4", NULL);
    SetEnvironmentVariableW(L"PATH", n ? path : NULL);

    for (i = 0; i < 32766; i++) cmd[i] = ' ';
    memcpy(cmd, L"prog.exe", 8 * sizeof(WCHAR));
    cmd[32765] = 'x'; cmd[32766] = 0;
    resolve("command-line-32766", NULL, cmd, NULL);
    cmd[32766] = 'x'; cmd[32767] = 0;
    resolve("command-line-32767", NULL, cmd, NULL);
    cmd[32767] = 'x'; cmd[32768] = 0;
    resolve("command-line-32768", NULL, cmd, NULL);

    SetEnvironmentVariableW(L"WD_MODE", L"quiet");
    u = WinExec("prog.exe", SW_HIDE);
    wd_emit("WinExec", "%s", u > 31 ? "started" : "failed");
    u = WinExec("nofile.exe", SW_HIDE);
    wd_emit("WinExec-missing", "%u", u);
    u = WinExec("notexe.exe", SW_HIDE);
    wd_emit("WinExec-text-file", "%u", u);
    u = WinExec("", SW_HIDE);
    wd_emit("WinExec-empty", "%u", u);
    Sleep(500);
    forward();
}

/* ---- the environment -------------------------------------------------- */

static void env_child(const char *id, const WCHAR *vars, DWORD flags, void *env)
{
    static WCHAR cmd[700];
    DWORD code;
    char cid[100];
    BOOL ok;

    set_child(id, L"env");
    SetEnvironmentVariableW(L"WD_VARS", vars);
    _snwprintf(cmd, 690, L"\"%ls\"", child_exe);
    ok = spawn(NULL, cmd, flags, env, NULL, NULL, TRUE, &code);
    _snprintf(cid, sizeof(cid), "%s.start", id);
    if (ok) wd_emit(cid, "started code=%lu", code);
    else wd_emit(cid, "e=%lu", GetLastError());
    forward();
}

/* An environment block: the WD_ variables the child needs, then `extra`
 * (entries separated by '|'). */
static int block_w(WCHAR *out, const char *id, const WCHAR *vars, const WCHAR *extra)
{
    int n = 0;
    WCHAR idw[200];
    const WCHAR *p;
    MultiByteToWideChar(CP_UTF8, 0, id, -1, idw, 190);
    n += _snwprintf(out + n, 700, L"WD_OUT=%ls", out_file) + 1;
    n += _snwprintf(out + n, 100, L"WD_EXE=proc") + 1;
    n += _snwprintf(out + n, 100, L"WD_GROUP=env") + 1;
    n += _snwprintf(out + n, 700, L"WD_CWD=%ls", wd_cwd) + 1;
    n += _snwprintf(out + n, 300, L"WD_ID=%ls", idw) + 1;
    n += _snwprintf(out + n, 100, L"WD_MODE=env") + 1;
    n += _snwprintf(out + n, 900, L"WD_VARS=%ls", vars) + 1;
    for (p = extra; *p; p++) out[n++] = (*p == '|') ? 0 : *p;
    out[n++] = 0; out[n++] = 0;
    return n;
}

static void env_block(const char *id, const WCHAR *vars, const WCHAR *extra, BOOL unicode, BOOL flag)
{
    static WCHAR wblock[4000], cmd[700];
    static char ablock[4000];
    DWORD code;
    char cid[100];
    BOOL ok;
    int n = block_w(wblock, id, vars, extra);

    if (!unicode) WideCharToMultiByte(CP_ACP, 0, wblock, n, ablock, sizeof(ablock), NULL, NULL);
    _snwprintf(cmd, 690, L"\"%ls\"", child_exe);
    ok = spawn(NULL, cmd, flag ? CREATE_UNICODE_ENVIRONMENT : 0, unicode ? (void *)wblock : (void *)ablock, NULL, NULL, TRUE, &code);
    _snprintf(cid, sizeof(cid), "%s.start", id);
    if (ok) wd_emit(cid, "started code=%lu", code);
    else wd_emit(cid, "e=%lu", GetLastError());
    forward();
}

static void g_env(void)
{
    static WCHAR big[40100], buf[40100];
    char abuf[64];
    DWORD n;
    BOOL ok;
    unsigned i;
    static const struct { const char *id; const WCHAR *s; } expand[] = {
        {"simple", L"%WD_A%"}, {"other-case", L"%wd_a%"}, {"unterminated", L"%WD_A"}, {"two-percent", L"%%"},
        {"one-percent", L"%"}, {"undefined", L"%WD_NOPE%"}, {"adjacent", L"%WD_A%%WD_A%"},
        {"double-percent-around", L"%%WD_A%%"}, {"percent-in-text", L"a%b"}, {"trailing-open", L"%WD_A%x%"},
        {"spaces-in-name", L"% WD_A %"}, {"empty-value", L"[%WD_EMPTY%]"}, {"empty-name", L"a%%b%%c"},
        {"nested", L"%WD_%WD_A%%"}, {"three-percent", L"%%%"}, {"value-with-percent", L"%WD_PCT%"},
        {"hidden-drive", L"%=C:%"}, {"plain", L"no variables"}, {"empty", L""},
    };
    char id[80];
    WCHAR *block, *p;
    int found = 0;

    init();
    EMIT_B("set", SetEnvironmentVariableW(L"WD_A", L"one"));
    SetLastError(0xdeadbeef);
    n = GetEnvironmentVariableW(L"wd_a", buf, 100);
    wd_emit("get-other-case", "%lu %s", n, wd_u8(buf));
    n = GetEnvironmentVariableW(L"WD_A", buf, 3);
    wd_emit("get-buffer-exact-length", "%lu", n);
    n = GetEnvironmentVariableW(L"WD_A", buf, 4);
    wd_emit("get-buffer-length-plus-one", "%lu", n);
    n = GetEnvironmentVariableW(L"WD_A", NULL, 0);
    wd_emit("get-size-query", "%lu", n);
    SetLastError(0xdeadbeef);
    n = GetEnvironmentVariableW(L"WD_NOPE", buf, 100);
    wd_emit("get-unset", "%lu gle=%lu", n, GetLastError());
    EMIT_B("set-empty-value", SetEnvironmentVariableW(L"WD_EMPTY", L""));
    SetLastError(0xdeadbeef);
    buf[0] = 'X';
    n = GetEnvironmentVariableW(L"WD_EMPTY", buf, 100);
    wd_emit("get-empty-value", "%lu gle=%lu buf0=%d", n, GetLastError(), buf[0]);
    EMIT_B("remove", SetEnvironmentVariableW(L"WD_A", NULL));
    EMIT_B("remove-unset", SetEnvironmentVariableW(L"WD_NOPE", NULL));
    EMIT_B("set-name-with-equals", SetEnvironmentVariableW(L"WD=X", L"1"));
    EMIT_B("set-name-starting-with-equals", SetEnvironmentVariableW(L"=WDX", L"1"));
    n = GetEnvironmentVariableW(L"=WDX", buf, 100);
    wd_emit("get-name-starting-with-equals", "%lu", n);
    EMIT_B("set-empty-name", SetEnvironmentVariableW(L"", L"1"));
    EMIT_B("set-null-name", SetEnvironmentVariableW(NULL, L"1"));
    EMIT_B("set-value-with-equals", SetEnvironmentVariableW(L"WD_EQ", L"a=b=c"));
    EMIT_B("set-name-with-space", SetEnvironmentVariableW(L"WD SPACE", L"1"));
    EMIT_B("set-name-lower-then-upper", (SetEnvironmentVariableW(L"wd_case", L"1"), SetEnvironmentVariableW(L"WD_CASE", L"2")));
    block = GetEnvironmentStringsW();
    for (p = block; p && *p; p += lstrlenW(p) + 1)
        if (!_wcsnicmp(p, L"wd_case=", 8)) { found++; wd_emit("block-keeps-first-spelling", "%s", wd_u8(p)); }
    wd_emit("block-entries-for-one-name", "%d", found);
    if (block) FreeEnvironmentStringsW(block);
    for (i = 0; i < 32766; i++) big[i] = 'v';
    big[32766] = 0;
    EMIT_B("set-value-32766", SetEnvironmentVariableW(L"WD_BIG", big));
    n = GetEnvironmentVariableW(L"WD_BIG", buf, 40000);
    wd_emit("get-value-32766", "%lu", n);
    for (i = 0; i < 40000; i++) big[i] = 'v';
    big[40000] = 0;
    EMIT_B("set-value-40000", SetEnvironmentVariableW(L"WD_BIG", big));
    n = GetEnvironmentVariableW(L"WD_BIG", buf, 40090);
    wd_emit("get-value-40000", "%lu", n);
    SetEnvironmentVariableW(L"WD_BIG", NULL);
    n = GetEnvironmentVariableA("WD_EQ", abuf, 3);
    wd_emit("getA-small-buffer", "%lu", n);

    SetEnvironmentVariableW(L"WD_A", L"one");
    SetEnvironmentVariableW(L"WD_PCT", L"%WD_A%");
    for (i = 0; i < ARRAY_SIZE(expand); i++)
    {
        _snprintf(id, sizeof(id), "expand.%s", expand[i].id);
        buf[0] = 0;
        n = ExpandEnvironmentStringsW(expand[i].s, buf, 1000);
        wd_emit(id, "%lu [%s]", n, wd_path(buf));
    }
    buf[0] = 'X'; buf[1] = 0;
    n = ExpandEnvironmentStringsW(L"%WD_A%", buf, 2);
    wd_emit("expand.small-buffer", "%lu buf0=%c", n, (char)buf[0]);
    n = ExpandEnvironmentStringsW(L"%WD_A%", NULL, 0);
    wd_emit("expand.size-query", "%lu", n);
    abuf[0] = 'X'; abuf[1] = 0;
    n = ExpandEnvironmentStringsA("%WD_A%", abuf, 2);
    wd_emit("expandA.small-buffer", "%lu buf0=%c", n, abuf[0]);
    n = ExpandEnvironmentStringsA("%WD_A%", abuf, 4);
    wd_emit("expandA.exact-buffer", "%lu", n);
    n = ExpandEnvironmentStringsA("%WD_A%", NULL, 0);
    wd_emit("expandA.size-query", "%lu", n);

    /* the C runtime's copy */
    wd_emit("crt.getenv-after-SetEnvironmentVariable", "%s", getenv("WD_A") ? getenv("WD_A") : "(null)");
    wd_emit("crt.putenv", "%d", _putenv("WD_CRT=two"));
    n = GetEnvironmentVariableW(L"WD_CRT", buf, 100);
    wd_emit("crt.putenv-visible-to-win32", "%lu", n);
    wd_emit("crt.putenv-remove", "%d", _putenv("WD_CRT="));
    SetLastError(0xdeadbeef);
    n = GetEnvironmentVariableW(L"WD_CRT", buf, 100);
    wd_emit("crt.removed-in-win32", "%lu gle=%lu", n, GetLastError());
    wd_emit("crt.putenv-no-equals", "%d", _putenv("WD_NOEQ"));
    wd_emit("crt.putenv-empty-name", "%d", _putenv("=x"));

    /* what a child sees */
    env_child("inherit", L"WD_A;WD_EMPTY;WD_EQ;wd_case;=WDX;WD SPACE;SystemRoot;WD_NOPE", 0, NULL);
    env_block("block.unsorted", L"A;B;SystemRoot;PATH", L"B=2|A=1", TRUE, TRUE);
    env_block("block.duplicate", L"A", L"A=1|A=2", TRUE, TRUE);
    env_block("block.duplicate-other-case", L"A;a", L"a=1|A=2", TRUE, TRUE);
    env_block("block.no-equals-entry", L"A;JUNK", L"JUNK|A=1", TRUE, TRUE);
    env_block("block.leading-equals-entry", L"A;=X", L"=X=1|A=1", TRUE, TRUE);
    env_block("block.empty-value", L"A;B", L"A=|B=2", TRUE, TRUE);
    env_block("block.ansi", L"A;B", L"B=2|A=1", FALSE, FALSE);
    env_block("block.ansi-with-unicode-flag", L"A;B", L"B=2|A=1", FALSE, TRUE);
    env_block("block.unicode-without-flag", L"A;B", L"B=2|A=1", TRUE, FALSE);
    (void)ok;
}

/* ---- exit codes ------------------------------------------------------- */

static void g_exit(void)
{
    static const WCHAR *kinds[] = {
        L"return0", L"return77", L"return-minus-1", L"exit5", L"_exit6", L"ExitProcess-big", L"ExitProcess-259",
        L"ExitProcess-ffffffff", L"TerminateProcess-self", L"abort", L"raise-SIGABRT", L"raise-SIGSEGV",
        L"raise-SIGTERM", L"raise-SIGINT", L"read-null", L"write-null", L"divide-by-zero", L"illegal-instruction",
        L"breakpoint", L"privileged-instruction", L"fastfail", L"DebugBreak", L"stack-overflow", L"RaiseException",
        L"RaiseException-noncontinuable", L"RaiseException-success-code", L"RaiseException-warning-code",
        L"filter-execute-handler", L"filter-exits-21", L"atexit-exits-22", L"ExitThread-main-only",
        L"ExitThread-main-other-exits-12", L"ExitThread-main-other-returns-13", L"return-while-thread-runs",
        L"crash-in-thread", L"TerminateThread-main", L"FatalExit", L"unknown-kind",
    };
    static WCHAR cmd[700], mode[100];
    unsigned i;
    DWORD code, tcode;
    char id[80];
    STARTUPINFOW si;
    PROCESS_INFORMATION pi;
    HANDLE h;

    init();
    _snwprintf(cmd, 690, L"\"%ls\"", child_exe);
    for (i = 0; i < ARRAY_SIZE(kinds); i++)
    {
        _snwprintf(mode, 99, L"exit:%ls", kinds[i]);
        set_child("x", mode);
        _snprintf(id, sizeof(id), "code.%s", wd_u8(kinds[i]));
        if (spawn(NULL, cmd, 0, NULL, NULL, NULL, TRUE, &code)) wd_emit(id, "0x%08lx", code);
        else wd_emit(id, "e=%lu", GetLastError());
    }

    /* a child that is still running */
    set_child("x", L"exit:sleep");
    memset(&si, 0, sizeof(si)); si.cb = sizeof(si);
    if (!CreateProcessW(NULL, cmd, NULL, NULL, FALSE, 0, NULL, NULL, &si, &pi)) { wd_emit("running.start", "e=%lu", GetLastError()); return; }
    Sleep(500);
    code = 0; GetExitCodeProcess(pi.hProcess, &code);
    tcode = 0; GetExitCodeThread(pi.hThread, &tcode);
    wd_emit("running.exit-codes", "process=%lu thread=%lu", code, tcode);
    wd_emit("running.wait-0", "%lu", WaitForSingleObject(pi.hProcess, 0));
    wd_emit("running.GetProcessId-matches", "%d", GetProcessId(pi.hProcess) == pi.dwProcessId);
    wd_emit("running.GetThreadId-matches", "%d", GetThreadId(pi.hThread) == pi.dwThreadId);
    wd_emit("running.GetProcessIdOfThread-matches", "%d", GetProcessIdOfThread(pi.hThread) == pi.dwProcessId);
    wd_emit("running.SuspendThread", "%lu", SuspendThread(pi.hThread));
    wd_emit("running.SuspendThread-again", "%lu", SuspendThread(pi.hThread));
    wd_emit("running.ResumeThread", "%lu", ResumeThread(pi.hThread));
    wd_emit("running.ResumeThread-again", "%lu", ResumeThread(pi.hThread));
    wd_emit("running.ResumeThread-not-suspended", "%lu", ResumeThread(pi.hThread));
    h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pi.dwProcessId);
    EMIT_B("running.OpenProcess-limited", h != NULL);
    EMIT_B("running.TerminateProcess-through-limited-handle", TerminateProcess(h, 1));
    if (h) CloseHandle(h);
    EMIT_B("running.TerminateProcess", TerminateProcess(pi.hProcess, 0x55));
    wd_emit("terminated.wait", "%lu", WaitForSingleObject(pi.hProcess, 10000));
    code = 0; GetExitCodeProcess(pi.hProcess, &code);
    tcode = 0; GetExitCodeThread(pi.hThread, &tcode);
    wd_emit("terminated.exit-codes", "process=0x%lx thread=0x%lx", code, tcode);
    EMIT_B("terminated.TerminateProcess-again", TerminateProcess(pi.hProcess, 0x66));
    code = 0; GetExitCodeProcess(pi.hProcess, &code);
    wd_emit("terminated.code-after-second-terminate", "0x%lx", code);
    h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pi.dwProcessId);
    EMIT_B("terminated.OpenProcess-while-handle-held", h != NULL);
    if (h) CloseHandle(h);
    wd_emit("terminated.ResumeThread", "%ld", (long)ResumeThread(pi.hThread));
    CloseHandle(pi.hThread); CloseHandle(pi.hProcess);
    h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pi.dwProcessId);
    EMIT_B("terminated.OpenProcess-after-handles-closed", h != NULL);
    if (h) CloseHandle(h);
    h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, 0);
    EMIT_B("OpenProcess-pid-0", h != NULL);
    if (h) CloseHandle(h);
    h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, 3);
    EMIT_B("OpenProcess-pid-3", h != NULL);
    if (h) CloseHandle(h);
    h = OpenProcess(0, FALSE, GetCurrentProcessId());
    EMIT_B("OpenProcess-no-access", h != NULL);
    if (h) CloseHandle(h);
    EMIT_B("GetExitCodeProcess-invalid-handle", GetExitCodeProcess(INVALID_HANDLE_VALUE, &code));
    EMIT_B("GetExitCodeProcess-null-handle", GetExitCodeProcess(NULL, &code));
    code = 0;
    EMIT_B("GetExitCodeProcess-current", GetExitCodeProcess(GetCurrentProcess(), &code));
    wd_emit("GetExitCodeProcess-current-value", "%lu", code);
    EMIT_B("GetExitCodeThread-process-handle", GetExitCodeThread(GetCurrentProcess(), &code));
}

/* ---- what a child is started with ------------------------------------- */

static void startup(const char *id, DWORD flags, STARTUPINFOW *psi, BOOL inherit, const WCHAR *dir)
{
    static WCHAR cmd[700];
    DWORD code;
    char cid[100];
    BOOL ok;

    set_child(id, L"startup");
    _snwprintf(cmd, 690, L"\"%ls\"", child_exe);
    ok = spawn(NULL, cmd, flags, NULL, dir, psi, inherit, &code);
    _snprintf(cid, sizeof(cid), "%s.start", id);
    if (ok) wd_emit(cid, "started code=%lu", code);
    else wd_emit(cid, "e=%lu", GetLastError());
    forward();
}

static void g_startup(void)
{
    STARTUPINFOW si;
    STARTUPINFOEXW six;
    HANDLE file, nul;
    SECURITY_ATTRIBUTES sa = {sizeof(sa), NULL, TRUE};
    WCHAR title[] = L"My Title", desktop[] = L"WinSta0\\Default";
    static WCHAR cmd[700];
    PROCESS_INFORMATION pi;

    init();
    CreateDirectoryW(L"sub", NULL);
    startup("default-inherit", 0, NULL, TRUE, NULL);
    startup("default-no-inherit", 0, NULL, FALSE, NULL);
    startup("detached", DETACHED_PROCESS, NULL, TRUE, NULL);
    startup("no-window", CREATE_NO_WINDOW, NULL, TRUE, NULL);
    startup("no-window-no-inherit", CREATE_NO_WINDOW, NULL, FALSE, NULL);
    startup("new-process-group", CREATE_NEW_PROCESS_GROUP, NULL, TRUE, NULL);
    startup("suspended", CREATE_SUSPENDED, NULL, TRUE, NULL);
    startup("idle-priority", IDLE_PRIORITY_CLASS, NULL, TRUE, NULL);
    startup("high-priority", HIGH_PRIORITY_CLASS, NULL, TRUE, NULL);
    startup("realtime-priority", REALTIME_PRIORITY_CLASS, NULL, TRUE, NULL);
    startup("two-priority-classes", IDLE_PRIORITY_CLASS | HIGH_PRIORITY_CLASS, NULL, TRUE, NULL);
    startup("detached-and-new-console", DETACHED_PROCESS | CREATE_NEW_CONSOLE, NULL, TRUE, NULL);
    startup("breakaway-without-job", CREATE_BREAKAWAY_FROM_JOB, NULL, TRUE, NULL);
    startup("default-error-mode", CREATE_DEFAULT_ERROR_MODE, NULL, TRUE, NULL);
    startup("directory-sub", 0, NULL, TRUE, L"sub");
    startup("directory-sub-trailing-backslash", 0, NULL, TRUE, L"sub\\");
    startup("directory-dotdot", 0, NULL, TRUE, L"sub\\..\\SUB");

    file = CreateFileW(L"stderr.txt", GENERIC_WRITE, WD_RWD, &sa, CREATE_ALWAYS, 0, NULL);
    nul = CreateFileW(L"NUL", GENERIC_READ, WD_RWD, &sa, OPEN_EXISTING, 0, NULL);
    memset(&si, 0, sizeof(si)); si.cb = sizeof(si);
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdInput = nul; si.hStdOutput = INVALID_HANDLE_VALUE; si.hStdError = file;
    startup("std-nul-invalid-file", 0, &si, TRUE, NULL);
    si.hStdInput = NULL; si.hStdOutput = NULL; si.hStdError = NULL;
    startup("std-all-null", 0, &si, TRUE, NULL);
    si.hStdInput = nul; si.hStdOutput = file; si.hStdError = file;
    startup("std-handles-not-inherited", 0, &si, FALSE, NULL);
    si.hStdInput = (HANDLE)(ULONG_PTR)0x7ffc; si.hStdOutput = file; si.hStdError = file;
    startup("std-bogus-handle-value", 0, &si, TRUE, NULL);
    si.hStdInput = GetCurrentProcess(); si.hStdOutput = file; si.hStdError = file;
    startup("std-pseudo-handle", 0, &si, TRUE, NULL);
    si.hStdInput = nul;
    startup("std-detached", DETACHED_PROCESS, &si, TRUE, NULL);
    memset(&si, 0, sizeof(si)); si.cb = sizeof(si);
    si.dwFlags = STARTF_USESHOWWINDOW | STARTF_USEPOSITION | STARTF_USESIZE | STARTF_FORCEONFEEDBACK;
    si.wShowWindow = SW_MINIMIZE; si.dwX = 11; si.dwY = 22; si.lpTitle = title; si.lpDesktop = desktop;
    startup("startupinfo-fields", 0, &si, TRUE, NULL);
    memset(&si, 0, sizeof(si)); si.cb = sizeof(si);
    si.wShowWindow = SW_MAXIMIZE; si.dwX = 11;
    startup("startupinfo-fields-without-flags", 0, &si, TRUE, NULL);
    memset(&si, 0, sizeof(si));
    startup("startupinfo-cb-0", 0, &si, TRUE, NULL);
    memset(&si, 0, sizeof(si)); si.cb = 8;
    startup("startupinfo-cb-8", 0, &si, TRUE, NULL);
    memset(&si, 0, sizeof(si)); si.cb = sizeof(si);
    startup("extended-flag-with-plain-startupinfo", EXTENDED_STARTUPINFO_PRESENT, &si, TRUE, NULL);
    memset(&six, 0, sizeof(six)); six.StartupInfo.cb = sizeof(six);
    startup("extended-startupinfo-null-list", EXTENDED_STARTUPINFO_PRESENT, &six.StartupInfo, TRUE, NULL);
    memset(&si, 0, sizeof(si)); si.cb = sizeof(si);
    si.dwFlags = STARTF_USESTDHANDLES | STARTF_USEHOTKEY;
    si.hStdInput = nul; si.hStdOutput = file; si.hStdError = file;
    startup("stdhandles-and-hotkey-flags", 0, &si, TRUE, NULL);
    memset(&si, 0, sizeof(si)); si.cb = sizeof(si);
    si.lpReserved2 = (BYTE *)"\0\0\0\0"; si.cbReserved2 = 4;
    startup("reserved2-zero-count", 0, &si, TRUE, NULL);
    CloseHandle(file); CloseHandle(nul);

    /* parameters CreateProcess refuses */
    _snwprintf(cmd, 690, L"\"%ls\"", child_exe);
    set_child("x", L"quiet");
    memset(&si, 0, sizeof(si)); si.cb = sizeof(si);
    EMIT_B("null-process-information", CreateProcessW(NULL, cmd, NULL, NULL, FALSE, 0, NULL, NULL, &si, NULL));
    EMIT_B("null-startupinfo", CreateProcessW(NULL, cmd, NULL, NULL, FALSE, 0, NULL, NULL, NULL, &pi));
}

static const struct wd_group_def groups[] = {
    {"cmdline", g_cmdline}, {"resolve", g_resolve}, {"env", g_env}, {"exit", g_exit}, {"startup", g_startup},
    {NULL, NULL},
};

int wmain(int argc, WCHAR **argv)
{
    return wd_main("proc", groups, argc, argv, NULL);
}
