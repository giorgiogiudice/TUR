/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * windiff, generated sequences: a pseudo-random walk over file operations
 * on a handful of names and handles, the same walk for the same seed on
 * every machine. Each step's outcome is recorded; a chunk of 20 steps is
 * one check. Windows is the oracle: the first chunk in which Wine's record
 * differs names the step at which behaviour diverged (the state differs
 * from there on, so only the first differing chunk of a sequence counts).
 */
#include "wd.h"

static unsigned int rng_state;
static unsigned int rnd(unsigned int n)
{
    rng_state ^= rng_state << 13;
    rng_state ^= rng_state >> 17;
    rng_state ^= rng_state << 5;
    return (rng_state >> 8) % n;
}

static const WCHAR *names[] = {L"a", L"b", L"c", L"d", L"d\\x", L"d\\y", L"e"};
static const char *short_names[] = {"a", "b", "c", "d", "d/x", "d/y", "e"};
#define NAMES 7
#define SLOTS 6
static HANDLE slots[SLOTS];

static int err(BOOL ok) { return ok ? 0 : (int)GetLastError(); }

static void sequence(unsigned int seed, int steps)
{
    static const DWORD acc[] = {GENERIC_READ, GENERIC_WRITE, GENERIC_READ | GENERIC_WRITE, DELETE, GENERIC_READ | DELETE,
                                FILE_READ_ATTRIBUTES, FILE_APPEND_DATA};
    static const DWORD shr[] = {0, FILE_SHARE_READ, FILE_SHARE_WRITE, WD_RW, WD_RWD, FILE_SHARE_DELETE, WD_RWD, WD_RWD};
    static const DWORD flg[] = {0, 0, 0, FILE_FLAG_DELETE_ON_CLOSE, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_HIDDEN,
                                FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_BACKUP_SEMANTICS};
    static const DWORD att[] = {FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL};
    static char chunk[4000];
    char id[40], buf[16];
    int step, n = 0, s, a, b, r, r2;
    DWORD got, v, access, share, disp, flags;
    struct { BOOLEAN del; } di;

    rng_state = seed * 2654435761u + 1;
    for (s = 0; s < SLOTS; s++) slots[s] = INVALID_HANDLE_VALUE;
    for (step = 0; step < steps; step++)
    {
        unsigned op = rnd(20);
        s = (int)rnd(SLOTS); a = (int)rnd(NAMES); b = (int)rnd(NAMES);
        switch (op)
        {
        case 0: case 1: case 2: case 3:
            access = acc[rnd(ARRAY_SIZE(acc))]; share = shr[rnd(ARRAY_SIZE(shr))];
            disp = 1 + rnd(5); flags = flg[rnd(ARRAY_SIZE(flg))];
            if (slots[s] != INVALID_HANDLE_VALUE) { CloseHandle(slots[s]); slots[s] = INVALID_HANDLE_VALUE; }
            slots[s] = CreateFileW(names[a], access, share, NULL, disp, flags, NULL);
            r = err(slots[s] != INVALID_HANDLE_VALUE);
            n += _snprintf(chunk + n, 100, "open(%s,%lx,%lx,%lu,%lx)>%d=%d ", short_names[a], access, share, disp, flags, s, r);
            break;
        case 4:
            if (slots[s] == INVALID_HANDLE_VALUE) { n += _snprintf(chunk + n, 40, "close(%d)=- ", s); break; }
            r = err(CloseHandle(slots[s])); slots[s] = INVALID_HANDLE_VALUE;
            n += _snprintf(chunk + n, 40, "close(%d)=%d ", s, r);
            break;
        case 5:
            got = 0; r = err(WriteFile(slots[s], "0123456789", 1 + rnd(9), &got, NULL));
            n += _snprintf(chunk + n, 40, "write(%d)=%d/%lu ", s, r, got);
            break;
        case 6:
            got = 0; r = err(ReadFile(slots[s], buf, 1 + rnd(9), &got, NULL));
            n += _snprintf(chunk + n, 40, "read(%d)=%d/%lu ", s, r, got);
            break;
        case 7: case 8:
            r = err(DeleteFileW(names[a]));
            n += _snprintf(chunk + n, 40, "del(%s)=%d ", short_names[a], r);
            break;
        case 9: case 10:
            v = rnd(2) ? MOVEFILE_REPLACE_EXISTING : 0;
            r = err(MoveFileExW(names[a], names[b], v));
            n += _snprintf(chunk + n, 60, "move(%s,%s,%lu)=%d ", short_names[a], short_names[b], v, r);
            break;
        case 11:
            v = att[rnd(ARRAY_SIZE(att))];
            r = err(SetFileAttributesW(names[a], v));
            n += _snprintf(chunk + n, 60, "attr(%s,%lx)=%d ", short_names[a], v, r);
            break;
        case 12:
            v = GetFileAttributesW(names[a]);
            if (v == INVALID_FILE_ATTRIBUTES) n += _snprintf(chunk + n, 60, "stat(%s)=e%lu ", short_names[a], GetLastError());
            else n += _snprintf(chunk + n, 60, "stat(%s)=%lx ", short_names[a], v & ~(DWORD)FILE_ATTRIBUTE_ARCHIVE);
            break;
        case 13:
            a = rnd(2) ? 3 : 6;
            if (rnd(2)) { r = err(CreateDirectoryW(names[a], NULL)); n += _snprintf(chunk + n, 60, "mkdir(%s)=%d ", short_names[a], r); }
            else { r = err(RemoveDirectoryW(names[a])); n += _snprintf(chunk + n, 60, "rmdir(%s)=%d ", short_names[a], r); }
            break;
        case 14:
            SetLastError(0);
            v = SetFilePointer(slots[s], (LONG)rnd(12), NULL, FILE_BEGIN);
            r = v == INVALID_SET_FILE_POINTER ? (int)GetLastError() : 0;
            r2 = err(SetEndOfFile(slots[s]));
            n += _snprintf(chunk + n, 60, "trunc(%d)=%d/%d ", s, r, r2);
            break;
        case 15:
            v = rnd(8);
            if (rnd(2)) { r = err(LockFile(slots[s], v, 0, 4, 0)); n += _snprintf(chunk + n, 60, "lock(%d,%lu)=%d ", s, v, r); }
            else { r = err(UnlockFile(slots[s], v, 0, 4, 0)); n += _snprintf(chunk + n, 60, "unlock(%d,%lu)=%d ", s, v, r); }
            break;
        case 16:
            v = rnd(2);
            r = err(CopyFileW(names[a], names[b], (BOOL)v));
            n += _snprintf(chunk + n, 60, "copy(%s,%s,%lu)=%d ", short_names[a], short_names[b], v, r);
            break;
        case 17:
            SetLastError(0);
            v = GetFileSize(slots[s], NULL);
            if (v == INVALID_FILE_SIZE) n += _snprintf(chunk + n, 60, "size(%d)=e%lu ", s, GetLastError());
            else n += _snprintf(chunk + n, 60, "size(%d)=%lu ", s, v);
            break;
        case 18:
            di.del = (BOOLEAN)rnd(2);
            r = err(SetFileInformationByHandle(slots[s], FileDispositionInfo, &di, sizeof(di)));
            n += _snprintf(chunk + n, 60, "disp(%d,%d)=%d ", s, di.del, r);
            break;
        default:
            r = err(CreateHardLinkW(names[b], names[a], NULL));
            n += _snprintf(chunk + n, 60, "link(%s,%s)=%d ", short_names[b], short_names[a], r);
            break;
        }
        if (step % 20 == 19 || step == steps - 1)
        {
            chunk[n] = 0;
            _snprintf(id, sizeof(id), "steps-%03d", step - step % 20);
            wd_emit(id, "%s", chunk);
            n = 0;
        }
    }
    for (s = 0; s < SLOTS; s++) if (slots[s] != INVALID_HANDLE_VALUE) CloseHandle(slots[s]);
    wd_emit("listing-at-end", "%s | d: %s | e: %s", wd_ls(L"."), wd_ls(L"d"), wd_ls(L"e"));
}

static void g_seq(void)
{
    /* the group is "seq-N": N is the seed */
    sequence((unsigned int)atoi(wd_group + 4), 400);
}

static const struct wd_group_def groups[] = {
    {"seq-1", g_seq}, {"seq-2", g_seq}, {"seq-3", g_seq}, {"seq-4", g_seq}, {"seq-5", g_seq}, {"seq-6", g_seq},
    {"seq-7", g_seq}, {"seq-8", g_seq}, {"seq-9", g_seq}, {"seq-10", g_seq}, {"seq-11", g_seq}, {"seq-12", g_seq},
    {NULL, NULL},
};

int wmain(int argc, WCHAR **argv)
{
    return wd_main("fuzz", groups, argc, argv, NULL);
}
