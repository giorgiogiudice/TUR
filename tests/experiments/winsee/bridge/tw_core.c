/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * tur-winbridge.exe: a small Windows program of TUR's own that runs inside
 * an application's Wine prefix, reads what the application's windows expose
 * and acts on them, for the experiment docs/reports/exp-winsee-1.md.
 *
 * It speaks one JSON object a line on its standard input and output, which
 * it inherits from its Linux parent. It holds no authority of its own: it
 * answers the parent's requests, and the parent (the experiment's adapter,
 * tests/experiments/winsee/harness/adapter.py, standing where tur-pax-a11y
 * would) asks only what the policy engine allowed.
 *
 * This file: the protocol, and the Win32 tier. A window is read through
 * what every Win32 program has: the window tree, classes, text, styles and
 * rectangles, and the standard control messages. Messages below WM_USER
 * are carried between processes by the system; the common controls' are
 * not, so their items are read through memory allocated in the
 * application's process (64-bit applications only here). A window's menu
 * cannot be read from another process under Wine, so it is read inside the
 * application by tur-winhook.dll (tw_hook.c).
 *
 * Requests: {"id":N,"op":"...", ...flat members...}
 * Answers:  {"id":N,"ok":true,"result":{...}}
 *           {"id":N,"ok":false,"error":{"code":"...","message":"..."}}
 */
#include "tw.h"

#include <fcntl.h>
#include <io.h>

/* ------------------------------------------------------------ buffers */

static void tw_reserve(tw_buf *b, size_t more)
{
    size_t cap;
    char *p;

    if (b->n + more + 1 <= b->cap)
        return;
    cap = b->cap ? b->cap * 2 : 4096;
    while (cap < b->n + more + 1)
        cap *= 2;
    p = realloc(b->p, cap);
    if (!p)
        ExitProcess(3);
    b->p = p;
    b->cap = cap;
}

void tw_putc(tw_buf *b, char c)
{
    tw_reserve(b, 1);
    b->p[b->n++] = c;
    b->p[b->n] = 0;
}

void tw_puts(tw_buf *b, const char *s)
{
    size_t length = strlen(s);

    tw_reserve(b, length);
    memcpy(b->p + b->n, s, length);
    b->n += length;
    b->p[b->n] = 0;
}

void tw_printf(tw_buf *b, const char *fmt, ...)
{
    char tmp[512];
    va_list ap;

    va_start(ap, fmt);
    tmp[0] = 0;
    vsnprintf(tmp, sizeof tmp, fmt, ap);
    va_end(ap);
    tmp[sizeof tmp - 1] = 0;
    tw_puts(b, tmp);
}

void tw_hex(tw_buf *b, ULONG_PTR v)
{
    char digits[2 * sizeof(ULONG_PTR) + 1];
    int i = (int)sizeof digits - 1;

    digits[i] = 0;
    do {
        digits[--i] = "0123456789abcdef"[v & 15];
        v >>= 4;
    } while (v && i > 0);
    tw_puts(b, digits + i);
}

void tw_hr(tw_buf *b, HRESULT hr)
{
    tw_puts(b, "\"0x");
    tw_hex(b, (ULONG_PTR)(DWORD)hr);
    tw_putc(b, '"');
}

void tw_jstr(tw_buf *b, const WCHAR *w)
{
    tw_putc(b, '"');
    if (w) {
        for (; *w; w++) {
            unsigned c = (unsigned)*w;

            if (c == '"' || c == '\\') {
                tw_putc(b, '\\');
                tw_putc(b, (char)c);
            } else if (c < 0x20 || c >= 0x7f) {
                tw_printf(b, "\\u%04x", c & 0xffff);
            } else {
                tw_putc(b, (char)c);
            }
        }
    }
    tw_putc(b, '"');
}

/* ------------------------------------------------------------- strings */

void tw_copy(WCHAR *dst, size_t cap, const WCHAR *src)
{
    size_t i = 0;

    if (!cap)
        return;
    if (src)
        for (; src[i] && i + 1 < cap; i++)
            dst[i] = src[i];
    dst[i] = 0;
}

void tw_cat(WCHAR *dst, size_t cap, const WCHAR *src)
{
    size_t have = (size_t)lstrlenW(dst);

    if (have < cap)
        tw_copy(dst + have, cap - have, src);
}

void tw_cat_hex(WCHAR *dst, size_t cap, ULONG_PTR v)
{
    WCHAR digits[2 * sizeof(ULONG_PTR) + 1];
    int i = (int)(sizeof digits / sizeof digits[0]) - 1;

    digits[i] = 0;
    do {
        digits[--i] = L"0123456789abcdef"[v & 15];
        v >>= 4;
    } while (v && i > 0);
    tw_cat(dst, cap, digits + i);
}

void tw_cat_int(WCHAR *dst, size_t cap, long v)
{
    WCHAR digits[24];
    int i = 23;
    unsigned long u = v < 0 ? (unsigned long)(-v) : (unsigned long)v;

    digits[i] = 0;
    do {
        digits[--i] = (WCHAR)(L'0' + u % 10);
        u /= 10;
    } while (u && i > 1);
    if (v < 0)
        digits[--i] = L'-';
    tw_cat(dst, cap, digits + i);
}

void tw_window_id(WCHAR *dst, size_t cap, HWND hwnd)
{
    tw_copy(dst, cap, L"h");
    tw_cat_hex(dst, cap, (ULONG_PTR)hwnd);
}

static WCHAR lower(WCHAR c)
{
    return (c >= 'A' && c <= 'Z') ? (WCHAR)(c + 32) : c;
}

/* Whether `hay` contains `needle`, ignoring the case of ASCII letters. */
static int contains(const WCHAR *hay, const WCHAR *needle)
{
    size_t i, j;

    if (!needle[0])
        return 1;
    for (i = 0; hay[i]; i++) {
        for (j = 0; needle[j] && hay[i + j] && lower(hay[i + j]) == lower(needle[j]); j++)
            ;
        if (!needle[j])
            return 1;
    }
    return 0;
}

static int same(const WCHAR *a, const WCHAR *b)
{
    return lstrcmpiW(a, b) == 0;
}

/* "&File" becomes "File", "&&" becomes "&"; what follows a tab (a menu
 * item's shortcut) is cut off and, if wanted, kept apart. */
static void strip_mnemonic(WCHAR *s, WCHAR *after_tab, size_t cap)
{
    WCHAR *r = s, *w = s;

    if (after_tab && cap)
        after_tab[0] = 0;
    while (*r) {
        if (*r == '&') {
            if (r[1] == '&') {
                *w++ = '&';
                r += 2;
            } else {
                r++;
            }
            continue;
        }
        if (*r == '\t') {
            if (after_tab)
                tw_copy(after_tab, cap, r + 1);
            break;
        }
        *w++ = *r++;
    }
    *w = 0;
}

static void trim_label(WCHAR *s)
{
    int n = lstrlenW(s);

    while (n > 0 && (s[n - 1] == ' ' || s[n - 1] == ':' || s[n - 1] == '\r' || s[n - 1] == '\n'))
        s[--n] = 0;
}

/* ------------------------------------------------------------ requests */

typedef struct {
    char key[32];
    WCHAR *value; /* NULL for an object or an array */
    int is_string;
} tw_field;

#define TW_FIELDS 24

static int hexval(char c)
{
    if (c >= '0' && c <= '9')
        return c - '0';
    if (c >= 'a' && c <= 'f')
        return c - 'a' + 10;
    if (c >= 'A' && c <= 'F')
        return c - 'A' + 10;
    return -1;
}

/* A JSON string starting after its opening quote; returns the position
 * after the closing quote, or NULL. `out` has room for the input's length. */
static const char *parse_string(const char *s, WCHAR *out)
{
    size_t n = 0;

    while (*s && *s != '"') {
        unsigned char c = (unsigned char)*s;

        if (c == '\\') {
            s++;
            switch (*s) {
            case 'n': out[n++] = '\n'; break;
            case 'r': out[n++] = '\r'; break;
            case 't': out[n++] = '\t'; break;
            case 'b': out[n++] = '\b'; break;
            case 'f': out[n++] = '\f'; break;
            case 'u': {
                int a = hexval(s[1]), b2, c2, d;

                if (a < 0)
                    return NULL;
                b2 = hexval(s[2]);
                if (b2 < 0)
                    return NULL;
                c2 = hexval(s[3]);
                if (c2 < 0)
                    return NULL;
                d = hexval(s[4]);
                if (d < 0)
                    return NULL;
                out[n++] = (WCHAR)((a << 12) | (b2 << 8) | (c2 << 4) | d);
                s += 4;
                break;
            }
            case 0:
                return NULL;
            default:
                out[n++] = (WCHAR)(unsigned char)*s;
                break;
            }
            s++;
        } else if (c < 0x80) {
            out[n++] = c;
            s++;
        } else {
            /* UTF-8 to UTF-16. */
            unsigned long cp;
            int extra;

            if ((c & 0xe0) == 0xc0) {
                cp = c & 0x1f;
                extra = 1;
            } else if ((c & 0xf0) == 0xe0) {
                cp = c & 0x0f;
                extra = 2;
            } else if ((c & 0xf8) == 0xf0) {
                cp = c & 0x07;
                extra = 3;
            } else {
                return NULL;
            }
            s++;
            while (extra-- > 0) {
                if (((unsigned char)*s & 0xc0) != 0x80)
                    return NULL;
                cp = (cp << 6) | ((unsigned char)*s & 0x3f);
                s++;
            }
            if (cp >= 0x10000) {
                cp -= 0x10000;
                out[n++] = (WCHAR)(0xd800 + (cp >> 10));
                out[n++] = (WCHAR)(0xdc00 + (cp & 0x3ff));
            } else {
                out[n++] = (WCHAR)cp;
            }
        }
    }
    if (*s != '"')
        return NULL;
    out[n] = 0;
    return s + 1;
}

static const char *skip_ws(const char *s)
{
    while (*s == ' ' || *s == '\t' || *s == '\r' || *s == '\n')
        s++;
    return s;
}

/* Skip an object or an array, strings included. */
static const char *skip_nested(const char *s)
{
    int depth = 0;

    for (; *s; s++) {
        if (*s == '"') {
            for (s++; *s && *s != '"'; s++)
                if (*s == '\\' && s[1])
                    s++;
            if (!*s)
                return NULL;
        } else if (*s == '{' || *s == '[') {
            depth++;
        } else if (*s == '}' || *s == ']') {
            if (--depth == 0)
                return s + 1;
        }
    }
    return NULL;
}

/* The members of one flat JSON object. Returns how many, or -1. */
static int parse_request(const char *s, tw_field *fields, int max)
{
    int count = 0;
    size_t length = strlen(s);

    s = skip_ws(s);
    if (*s != '{')
        return -1;
    s = skip_ws(s + 1);
    while (*s && *s != '}') {
        tw_field field;
        size_t k = 0;

        memset(&field, 0, sizeof field);
        if (*s != '"')
            return -1;
        for (s++; *s && *s != '"'; s++)
            if (k + 1 < sizeof field.key)
                field.key[k++] = *s;
        if (*s != '"')
            return -1;
        s = skip_ws(s + 1);
        if (*s != ':')
            return -1;
        s = skip_ws(s + 1);
        if (*s == '"') {
            field.value = calloc(length + 2, sizeof(WCHAR));
            if (!field.value)
                return -1;
            field.is_string = 1;
            s = parse_string(s + 1, field.value);
            if (!s)
                return -1;
        } else if (*s == '{' || *s == '[') {
            s = skip_nested(s);
            if (!s)
                return -1;
        } else {
            size_t n = 0;

            field.value = calloc(64, sizeof(WCHAR));
            if (!field.value)
                return -1;
            while (*s && *s != ',' && *s != '}' && *s != ' ' && *s != '\r' && *s != '\n') {
                if (n < 62)
                    field.value[n++] = (WCHAR)(unsigned char)*s;
                s++;
            }
        }
        if (count < max)
            fields[count++] = field;
        else
            free(field.value);
        s = skip_ws(s);
        if (*s == ',')
            s = skip_ws(s + 1);
    }
    return *s == '}' ? count : -1;
}

static const WCHAR *field_text(const tw_field *fields, int count, const char *key)
{
    int i;

    for (i = 0; i < count; i++)
        if (!strcmp(fields[i].key, key) && fields[i].value)
            return fields[i].value;
    return NULL;
}

static long field_int(const tw_field *fields, int count, const char *key, long fallback)
{
    const WCHAR *text = field_text(fields, count, key);

    if (!text || !text[0])
        return fallback;
    return wcstol(text, NULL, 10);
}

/* --------------------------------------------------------------- nodes */

static const struct {
    int role;
    const char *name;
} role_names[] = {
    {TW_ROLE_CHECK_BOX, "check box"}, {TW_ROLE_CHECK_MENU_ITEM, "check menu item"},
    {TW_ROLE_COMBO_BOX, "combo box"}, {TW_ROLE_DIALOG, "dialog"}, {TW_ROLE_FRAME, "frame"},
    {TW_ROLE_IMAGE, "image"}, {TW_ROLE_LABEL, "label"}, {TW_ROLE_LIST, "list"},
    {TW_ROLE_LIST_ITEM, "list item"}, {TW_ROLE_MENU, "menu"}, {TW_ROLE_MENU_BAR, "menu bar"},
    {TW_ROLE_MENU_ITEM, "menu item"}, {TW_ROLE_PAGE_TAB, "page tab"},
    {TW_ROLE_PAGE_TAB_LIST, "page tab list"}, {TW_ROLE_PANEL, "panel"},
    {TW_ROLE_PASSWORD_TEXT, "password text"}, {TW_ROLE_PROGRESS_BAR, "progress bar"},
    {TW_ROLE_PUSH_BUTTON, "push button"}, {TW_ROLE_RADIO_BUTTON, "radio button"},
    {TW_ROLE_SCROLL_BAR, "scroll bar"}, {TW_ROLE_SEPARATOR, "separator"},
    {TW_ROLE_SLIDER, "slider"}, {TW_ROLE_SPIN_BUTTON, "spin button"},
    {TW_ROLE_STATUS_BAR, "status bar"}, {TW_ROLE_TABLE_COLUMN_HEADER, "table column header"},
    {TW_ROLE_TERMINAL, "terminal"}, {TW_ROLE_TEXT, "text"},
    {TW_ROLE_TOGGLE_BUTTON, "toggle button"}, {TW_ROLE_TOOL_BAR, "tool bar"},
    {TW_ROLE_TOOL_TIP, "tool tip"}, {TW_ROLE_TREE, "tree"}, {TW_ROLE_UNKNOWN, "unknown"},
    {TW_ROLE_WINDOW, "window"}, {TW_ROLE_LINK, "link"}, {TW_ROLE_TREE_ITEM, "tree item"},
    {TW_ROLE_TITLE_BAR, "title bar"},
};

const char *tw_role_name(int role)
{
    size_t i;

    for (i = 0; i < sizeof role_names / sizeof role_names[0]; i++)
        if (role_names[i].role == role)
            return role_names[i].name;
    return "unknown";
}

tw_node *tw_add(tw_nodes *ns)
{
    tw_node *node;

    ns->visited++;
    if (ns->n >= ns->max || ns->n >= ns->cap) {
        ns->truncated = 1;
        return NULL;
    }
    node = &ns->v[ns->n++];
    memset(node, 0, sizeof *node);
    node->item = -1;
    node->sel_start = node->sel_end = -1;
    node->role = TW_ROLE_UNKNOWN;
    return node;
}

static int nodes_init(tw_nodes *ns, int max)
{
    memset(ns, 0, sizeof *ns);
    if (max < 10)
        max = 10;
    if (max > 4000)
        max = 4000;
    ns->v = calloc((size_t)max, sizeof(tw_node));
    if (!ns->v)
        return 0;
    ns->cap = max;
    ns->max = max;
    return 1;
}

static void nodes_free(tw_nodes *ns)
{
    free(ns->v);
    ns->v = NULL;
}

static tw_node *find_node(tw_nodes *ns, const WCHAR *id)
{
    int i;

    for (i = 0; i < ns->n; i++)
        if (!lstrcmpW(ns->v[i].id, id))
            return &ns->v[i];
    return NULL;
}

static void put_node(tw_buf *b, const tw_node *n, const RECT *top)
{
    static const struct {
        unsigned bit;
        const char *name;
    } states[] = {
        {TW_ST_DISABLED, "disabled"}, {TW_ST_FOCUSED, "focused"}, {TW_ST_CHECKED, "checked"},
        {TW_ST_SELECTED, "selected"}, {TW_ST_EXPANDED, "expanded"}, {TW_ST_EDITABLE, "editable"},
        {TW_ST_DEFAULT, "is default"}, {TW_ST_READONLY, "read only"},
        {TW_ST_OFFSCREEN, "offscreen"}, {TW_ST_EXPANDABLE, "expandable"},
        {TW_ST_MULTILINE, "multi line"},
    }, actions[] = {
        {TW_ACT_PRESS, "press"}, {TW_ACT_TYPE, "type"}, {TW_ACT_SELECT, "select"},
        {TW_ACT_EXPAND, "expand"}, {TW_ACT_COLLAPSE, "collapse"}, {TW_ACT_FOCUS, "focus"},
        {TW_ACT_SCROLL, "scroll"},
    };
    size_t i;
    int first;

    tw_puts(b, "{\"id\":");
    tw_jstr(b, n->id);
    tw_puts(b, ",\"parent\":");
    tw_jstr(b, n->parent);
    tw_printf(b, ",\"depth\":%d,\"role\":\"%s\",\"role_id\":%d", n->depth, tw_role_name(n->role), n->role);
    if (n->native_role)
        tw_printf(b, ",\"native_role\":%d", n->native_role);
    if (n->name[0]) {
        tw_puts(b, ",\"name\":");
        tw_jstr(b, n->name);
    }
    if (n->name_from)
        tw_printf(b, ",\"name_from\":\"%s\"", n->name_from);
    if (n->description[0]) {
        tw_puts(b, ",\"description\":");
        tw_jstr(b, n->description);
    }
    if (n->cls[0]) {
        tw_puts(b, ",\"class\":");
        tw_jstr(b, n->cls);
    }
    tw_puts(b, ",\"states\":[");
    first = 1;
    for (i = 0; i < sizeof states / sizeof states[0]; i++)
        if (n->states & states[i].bit) {
            tw_printf(b, "%s\"%s\"", first ? "" : ",", states[i].name);
            first = 0;
        }
    tw_puts(b, "],\"actions\":[");
    first = 1;
    for (i = 0; i < sizeof actions / sizeof actions[0]; i++)
        if (n->actions & actions[i].bit) {
            tw_printf(b, "%s\"%s\"", first ? "" : ",", actions[i].name);
            first = 0;
        }
    tw_putc(b, ']');
    if (n->has_rc) {
        tw_printf(b, ",\"screen\":[%ld,%ld,%ld,%ld]", (long)n->rc.left, (long)n->rc.top,
                  (long)(n->rc.right - n->rc.left), (long)(n->rc.bottom - n->rc.top));
        if (top)
            tw_printf(b, ",\"bounds\":[%ld,%ld,%ld,%ld]", (long)(n->rc.left - top->left),
                      (long)(n->rc.top - top->top), (long)(n->rc.right - n->rc.left),
                      (long)(n->rc.bottom - n->rc.top));
    }
    if (n->text_withheld)
        tw_puts(b, ",\"text_withheld\":true");
    else if (n->has_text) {
        tw_puts(b, ",\"text\":");
        tw_jstr(b, n->text);
    }
    if (n->sel_start >= 0)
        tw_printf(b, ",\"selection\":[%d,%d]", n->sel_start, n->sel_end);
    if (n->kind == 'm' || n->kind == 'b')
        tw_printf(b, ",\"command\":%u", n->command);
    tw_putc(b, '}');
}

/* ---------------------------------------------------------- Win32 tier */

static LRESULT smsg(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam)
{
    DWORD_PTR result = 0;

    if (!SendMessageTimeoutW(hwnd, message, wparam, lparam, SMTO_ABORTIFHUNG, 1500, &result))
        return 0;
    return (LRESULT)result;
}

static void window_text(HWND hwnd, WCHAR *buffer, int cap)
{
    buffer[0] = 0;
    smsg(hwnd, WM_GETTEXT, (WPARAM)cap, (LPARAM)buffer);
    buffer[cap - 1] = 0;
}

static int is_static_class(const WCHAR *cls)
{
    return same(cls, L"Static") || contains(cls, L"Static") || contains(cls, L"Label");
}

static int is_edit_class(const WCHAR *cls)
{
    return same(cls, L"Edit") || contains(cls, L"Edit") || contains(cls, L"Memo");
}

/* Memory in the application's process, for the common controls' messages
 * that carry a pointer. Only for an application of the bridge's own word
 * size. */
typedef struct {
    HANDLE process;
    BYTE *memory;
    int ok;
    int wow64;
} tw_remote;

#define TW_REMOTE_SIZE 16384
#define TW_REMOTE_TEXT 4096 /* where the text buffer starts */
#define TW_REMOTE_CHARS 1024

static int remote_open(tw_remote *r, HWND hwnd)
{
    DWORD pid = 0;
    BOOL wow = FALSE;

    memset(r, 0, sizeof *r);
    GetWindowThreadProcessId(hwnd, &pid);
    if (!pid)
        return 0;
    r->process = OpenProcess(PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE |
                                 PROCESS_QUERY_INFORMATION,
                             FALSE, pid);
    if (!r->process)
        return 0;
    if (IsWow64Process(r->process, &wow) && wow) {
        r->wow64 = 1;
        CloseHandle(r->process);
        r->process = NULL;
        return 0;
    }
    r->memory = VirtualAllocEx(r->process, NULL, TW_REMOTE_SIZE, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (!r->memory) {
        CloseHandle(r->process);
        r->process = NULL;
        return 0;
    }
    r->ok = 1;
    return 1;
}

static void remote_close(tw_remote *r)
{
    if (r->process) {
        if (r->memory)
            VirtualFreeEx(r->process, r->memory, 0, MEM_RELEASE);
        CloseHandle(r->process);
    }
    memset(r, 0, sizeof *r);
}

static int remote_put(tw_remote *r, const void *data, SIZE_T size)
{
    SIZE_T done = 0;

    return r->ok && WriteProcessMemory(r->process, r->memory, data, size, &done) && done == size;
}

static int remote_get(tw_remote *r, SIZE_T offset, void *data, SIZE_T size)
{
    SIZE_T done = 0;

    return r->ok && ReadProcessMemory(r->process, r->memory + offset, data, size, &done) && done == size;
}

/* The text the last message wrote into the remote text buffer. */
static void remote_text(tw_remote *r, WCHAR *out, size_t cap)
{
    WCHAR buffer[TW_REMOTE_CHARS];

    out[0] = 0;
    memset(buffer, 0, sizeof buffer);
    if (remote_get(r, TW_REMOTE_TEXT, buffer, sizeof buffer)) {
        buffer[TW_REMOTE_CHARS - 1] = 0;
        tw_copy(out, cap, buffer);
    }
}

static void remote_clear_text(tw_remote *r)
{
    WCHAR zero = 0;
    SIZE_T done;

    if (r->ok)
        WriteProcessMemory(r->process, r->memory + TW_REMOTE_TEXT, &zero, sizeof zero, &done);
}

static void to_screen(HWND hwnd, RECT *rc)
{
    MapWindowPoints(hwnd, NULL, (POINT *)rc, 2);
}

static int overlaps(const RECT *a, const RECT *b)
{
    RECT both;

    return IntersectRect(&both, a, b);
}

/* A new node for item `index` of the control `ctl`. */
static tw_node *add_item(tw_nodes *ns, const tw_node *ctl, int index, char kind, char which, int role)
{
    tw_node *node = tw_add(ns);

    if (!node)
        return NULL;
    tw_copy(node->id, TW_ID, ctl->id);
    tw_cat(node->id, TW_ID, kind == 'b' ? L"/b" : L"/i");
    tw_cat_int(node->id, TW_ID, index);
    tw_copy(node->parent, TW_ID, ctl->id);
    node->depth = ctl->depth + 1;
    node->role = role;
    node->kind = kind;
    node->ctl = which;
    node->hwnd = ctl->hwnd;
    node->item = index;
    node->name_from = "item";
    tw_copy(node->cls, 64, ctl->cls);
    if (ctl->states & TW_ST_DISABLED)
        node->states |= TW_ST_DISABLED;
    return node;
}

static void item_rect(tw_node *node, HWND ctl, RECT rc)
{
    RECT client;

    GetClientRect(ctl, &client);
    if (rc.right <= rc.left || rc.bottom <= rc.top)
        return;
    if (!overlaps(&rc, &client))
        node->states |= TW_ST_OFFSCREEN;
    to_screen(ctl, &rc);
    node->rc = rc;
    node->has_rc = 1;
}

static void items_listbox(tw_nodes *ns, const tw_node *ctl, LONG style)
{
    HWND h = ctl->hwnd;
    int count = (int)smsg(h, LB_GETCOUNT, 0, 0), current = (int)smsg(h, LB_GETCURSEL, 0, 0), i;
    int strings = !(style & (LBS_OWNERDRAWFIXED | LBS_OWNERDRAWVARIABLE)) || (style & LBS_HASSTRINGS);

    for (i = 0; i < count && i < 60; i++) {
        tw_node *node = add_item(ns, ctl, i, 'i', 'L', TW_ROLE_LIST_ITEM);
        WCHAR text[1024];
        RECT rc;
        LRESULT length;

        if (!node)
            return;
        text[0] = 0;
        length = smsg(h, LB_GETTEXTLEN, (WPARAM)i, 0);
        if (strings && length > 0 && length < 1000)
            smsg(h, LB_GETTEXT, (WPARAM)i, (LPARAM)text);
        text[1023] = 0;
        tw_copy(node->name, TW_NAME, text);
        node->actions = TW_ACT_SELECT | TW_ACT_SCROLL;
        if (i == current)
            node->states |= TW_ST_SELECTED;
        SetRectEmpty(&rc);
        if (smsg(h, LB_GETITEMRECT, (WPARAM)i, (LPARAM)&rc) != LB_ERR)
            item_rect(node, h, rc);
    }
}

static void items_combo(tw_nodes *ns, const tw_node *ctl, LONG style)
{
    HWND h = ctl->hwnd;
    int count = (int)smsg(h, CB_GETCOUNT, 0, 0), current = (int)smsg(h, CB_GETCURSEL, 0, 0), i;
    int strings = !(style & (CBS_OWNERDRAWFIXED | CBS_OWNERDRAWVARIABLE)) || (style & CBS_HASSTRINGS);

    for (i = 0; i < count && i < 40; i++) {
        tw_node *node = add_item(ns, ctl, i, 'i', 'C', TW_ROLE_LIST_ITEM);
        WCHAR text[1024];
        LRESULT length;

        if (!node)
            return;
        text[0] = 0;
        length = smsg(h, CB_GETLBTEXTLEN, (WPARAM)i, 0);
        if (strings && length > 0 && length < 1000)
            smsg(h, CB_GETLBTEXT, (WPARAM)i, (LPARAM)text);
        text[1023] = 0;
        tw_copy(node->name, TW_NAME, text);
        node->actions = TW_ACT_SELECT;
        /* The list of a combo box is shown only when it is dropped. */
        node->states |= TW_ST_OFFSCREEN;
        if (i == current)
            node->states |= TW_ST_SELECTED;
    }
}

static void items_listview(tw_nodes *ns, const tw_node *ctl, tw_remote *r)
{
    HWND h = ctl->hwnd;
    int count = (int)smsg(h, LVM_GETITEMCOUNT, 0, 0), i;

    for (i = 0; i < count && i < 60; i++) {
        tw_node *node = add_item(ns, ctl, i, 'i', 'V', TW_ROLE_LIST_ITEM);
        LVITEMW item;
        RECT rc;

        if (!node)
            return;
        node->actions = TW_ACT_SELECT | TW_ACT_SCROLL;
        if (smsg(h, LVM_GETITEMSTATE, (WPARAM)i, LVIS_SELECTED) & LVIS_SELECTED)
            node->states |= TW_ST_SELECTED;
        if (!r->ok)
            continue;
        memset(&item, 0, sizeof item);
        item.iSubItem = 0;
        item.pszText = (WCHAR *)(r->memory + TW_REMOTE_TEXT);
        item.cchTextMax = TW_REMOTE_CHARS - 1;
        remote_clear_text(r);
        if (remote_put(r, &item, sizeof item)) {
            smsg(h, LVM_GETITEMTEXTW, (WPARAM)i, (LPARAM)r->memory);
            remote_text(r, node->name, TW_NAME);
        }
        item.iSubItem = 1;
        remote_clear_text(r);
        if (remote_put(r, &item, sizeof item)) {
            smsg(h, LVM_GETITEMTEXTW, (WPARAM)i, (LPARAM)r->memory);
            remote_text(r, node->description, TW_NAME);
        }
        SetRectEmpty(&rc);
        rc.left = LVIR_BOUNDS;
        if (remote_put(r, &rc, sizeof rc) && smsg(h, LVM_GETITEMRECT, (WPARAM)i, (LPARAM)r->memory) &&
            remote_get(r, 0, &rc, sizeof rc))
            item_rect(node, h, rc);
    }
}

static void items_header(tw_nodes *ns, const tw_node *ctl, tw_remote *r)
{
    HWND h = ctl->hwnd;
    int count = (int)smsg(h, HDM_GETITEMCOUNT, 0, 0), i;

    for (i = 0; i < count && i < 30; i++) {
        tw_node *node = add_item(ns, ctl, i, 'i', 'H', TW_ROLE_TABLE_COLUMN_HEADER);
        HDITEMW item;
        RECT rc;

        if (!node)
            return;
        if (!r->ok)
            continue;
        memset(&item, 0, sizeof item);
        item.mask = HDI_TEXT;
        item.pszText = (WCHAR *)(r->memory + TW_REMOTE_TEXT);
        item.cchTextMax = TW_REMOTE_CHARS - 1;
        remote_clear_text(r);
        if (remote_put(r, &item, sizeof item)) {
            smsg(h, HDM_GETITEMW, (WPARAM)i, (LPARAM)r->memory);
            remote_text(r, node->name, TW_NAME);
        }
        SetRectEmpty(&rc);
        if (remote_put(r, &rc, sizeof rc) && smsg(h, HDM_GETITEMRECT, (WPARAM)i, (LPARAM)r->memory) &&
            remote_get(r, 0, &rc, sizeof rc))
            item_rect(node, h, rc);
    }
}

static void items_tab(tw_nodes *ns, const tw_node *ctl, tw_remote *r)
{
    HWND h = ctl->hwnd;
    int count = (int)smsg(h, TCM_GETITEMCOUNT, 0, 0), current = (int)smsg(h, TCM_GETCURSEL, 0, 0), i;

    for (i = 0; i < count && i < 40; i++) {
        tw_node *node = add_item(ns, ctl, i, 'i', 'T', TW_ROLE_PAGE_TAB);
        TCITEMW item;
        RECT rc;

        if (!node)
            return;
        node->actions = TW_ACT_SELECT;
        if (i == current)
            node->states |= TW_ST_SELECTED;
        if (!r->ok)
            continue;
        memset(&item, 0, sizeof item);
        item.mask = TCIF_TEXT;
        item.pszText = (WCHAR *)(r->memory + TW_REMOTE_TEXT);
        item.cchTextMax = TW_REMOTE_CHARS - 1;
        remote_clear_text(r);
        if (remote_put(r, &item, sizeof item)) {
            smsg(h, TCM_GETITEMW, (WPARAM)i, (LPARAM)r->memory);
            remote_text(r, node->name, TW_NAME);
            strip_mnemonic(node->name, NULL, 0);
        }
        SetRectEmpty(&rc);
        if (remote_put(r, &rc, sizeof rc) && smsg(h, TCM_GETITEMRECT, (WPARAM)i, (LPARAM)r->memory) &&
            remote_get(r, 0, &rc, sizeof rc))
            item_rect(node, h, rc);
    }
}

static void items_toolbar(tw_nodes *ns, const tw_node *ctl, tw_remote *r)
{
    HWND h = ctl->hwnd;
    int count = (int)smsg(h, TB_BUTTONCOUNT, 0, 0), i;

    if (!r->ok)
        return;
    for (i = 0; i < count && i < 80; i++) {
        TBBUTTON button;
        tw_node *node;
        RECT rc;

        memset(&button, 0, sizeof button);
        if (!remote_put(r, &button, sizeof button) || !smsg(h, TB_GETBUTTON, (WPARAM)i, (LPARAM)r->memory) ||
            !remote_get(r, 0, &button, sizeof button))
            continue;
        if ((button.fsStyle & BTNS_SEP) || (button.fsState & TBSTATE_HIDDEN))
            continue;
        node = add_item(ns, ctl, i, 'b', 0, (button.fsStyle & BTNS_CHECK) ? TW_ROLE_TOGGLE_BUTTON : TW_ROLE_PUSH_BUTTON);
        if (!node)
            return;
        node->command = (UINT)button.idCommand;
        node->actions = TW_ACT_PRESS;
        if (!(button.fsState & TBSTATE_ENABLED))
            node->states |= TW_ST_DISABLED;
        if (button.fsState & TBSTATE_CHECKED)
            node->states |= TW_ST_CHECKED;
        remote_clear_text(r);
        if ((int)smsg(h, TB_GETBUTTONTEXTW, (WPARAM)button.idCommand, (LPARAM)(r->memory + TW_REMOTE_TEXT)) > 0) {
            remote_text(r, node->name, TW_NAME);
            strip_mnemonic(node->name, NULL, 0);
        }
        if (!node->name[0])
            node->name_from = "none";
        SetRectEmpty(&rc);
        if (remote_put(r, &rc, sizeof rc) && smsg(h, TB_GETITEMRECT, (WPARAM)i, (LPARAM)r->memory) &&
            remote_get(r, 0, &rc, sizeof rc))
            item_rect(node, h, rc);
    }
}

static void items_status(tw_nodes *ns, const tw_node *ctl, tw_remote *r)
{
    HWND h = ctl->hwnd;
    int count = (int)smsg(h, SB_GETPARTS, 0, 0), i;

    for (i = 0; i < count && i < 20; i++) {
        tw_node *node = add_item(ns, ctl, i, 'i', 'S', TW_ROLE_LABEL);
        RECT rc;

        if (!node)
            return;
        if (!r->ok)
            continue;
        remote_clear_text(r);
        if (LOWORD(smsg(h, SB_GETTEXTLENGTHW, (WPARAM)i, 0)) < TW_REMOTE_CHARS - 1) {
            smsg(h, SB_GETTEXTW, (WPARAM)i, (LPARAM)(r->memory + TW_REMOTE_TEXT));
            remote_text(r, node->name, TW_NAME);
        }
        SetRectEmpty(&rc);
        if (remote_put(r, &rc, sizeof rc) && smsg(h, SB_GETRECT, (WPARAM)i, (LPARAM)r->memory) &&
            remote_get(r, 0, &rc, sizeof rc))
            item_rect(node, h, rc);
    }
}

static void tree_items(tw_nodes *ns, const tw_node *ctl, tw_remote *r, HTREEITEM first, const WCHAR *parent_id,
                       int depth, int *budget)
{
    HWND h = ctl->hwnd;
    HTREEITEM item;

    for (item = first; item && *budget > 0;
         item = (HTREEITEM)smsg(h, TVM_GETNEXTITEM, TVGN_NEXT, (LPARAM)item)) {
        tw_node *node = tw_add(ns);
        TVITEMW info;
        RECT rc;
        HTREEITEM child;

        if (!node)
            return;
        (*budget)--;
        tw_copy(node->id, TW_ID, ctl->id);
        tw_cat(node->id, TW_ID, L"/t");
        tw_cat_hex(node->id, TW_ID, (ULONG_PTR)item);
        tw_copy(node->parent, TW_ID, parent_id);
        node->depth = depth;
        node->role = TW_ROLE_TREE_ITEM;
        node->kind = 't';
        node->hwnd = h;
        node->handle = (ULONG_PTR)item;
        node->name_from = "item";
        node->actions = TW_ACT_SELECT | TW_ACT_SCROLL;
        tw_copy(node->cls, 64, ctl->cls);
        memset(&info, 0, sizeof info);
        info.mask = TVIF_TEXT | TVIF_STATE | TVIF_CHILDREN | TVIF_HANDLE;
        info.hItem = item;
        info.stateMask = TVIS_EXPANDED | TVIS_SELECTED;
        info.pszText = (WCHAR *)(r->memory + TW_REMOTE_TEXT);
        info.cchTextMax = TW_REMOTE_CHARS - 1;
        remote_clear_text(r);
        if (remote_put(r, &info, sizeof info) && smsg(h, TVM_GETITEMW, 0, (LPARAM)r->memory) &&
            remote_get(r, 0, &info, sizeof info)) {
            remote_text(r, node->name, TW_NAME);
            if (info.state & TVIS_SELECTED)
                node->states |= TW_ST_SELECTED;
            if (info.state & TVIS_EXPANDED)
                node->states |= TW_ST_EXPANDED;
            if (info.cChildren) {
                node->states |= TW_ST_EXPANDABLE;
                node->actions |= TW_ACT_EXPAND | TW_ACT_COLLAPSE;
            }
        }
        SetRectEmpty(&rc);
        /* TVM_GETITEMRECT takes the item's handle in the rectangle's place. */
        memcpy(&rc, &item, sizeof item);
        if (remote_put(r, &rc, sizeof rc) && smsg(h, TVM_GETITEMRECT, TRUE, (LPARAM)r->memory) &&
            remote_get(r, 0, &rc, sizeof rc))
            item_rect(node, h, rc);
        else
            node->states |= TW_ST_OFFSCREEN;
        child = (HTREEITEM)smsg(h, TVM_GETNEXTITEM, TVGN_CHILD, (LPARAM)item);
        if (child && (node->states & TW_ST_EXPANDED)) {
            WCHAR id[TW_ID];

            tw_copy(id, TW_ID, node->id);
            tree_items(ns, ctl, r, child, id, depth + 1, budget);
        }
    }
}

/* The label of a control that has no text of its own: the nearest visible
 * static control before it among its siblings, as Windows' own accessible
 * objects do it; else the nearest one to its left or above it. */
static const char *find_label(HWND hwnd, WCHAR *out, size_t cap)
{
    HWND other, parent = GetParent(hwnd), best = NULL;
    RECT mine;
    long best_distance = 1000000;
    WCHAR cls[64], text[TW_NAME];

    out[0] = 0;
    for (other = GetWindow(hwnd, GW_HWNDPREV); other; other = GetWindow(other, GW_HWNDPREV)) {
        if (!IsWindowVisible(other))
            continue;
        cls[0] = 0;
        GetClassNameW(other, cls, 64);
        if (!is_static_class(cls))
            break;
        window_text(other, text, TW_NAME);
        strip_mnemonic(text, NULL, 0);
        trim_label(text);
        if (text[0]) {
            tw_copy(out, cap, text);
            return "label";
        }
    }
    if (!parent || !GetWindowRect(hwnd, &mine))
        return NULL;
    for (other = GetWindow(parent, GW_CHILD); other; other = GetWindow(other, GW_HWNDNEXT)) {
        RECT rc;
        long distance = -1;

        if (other == hwnd || !IsWindowVisible(other) || !GetWindowRect(other, &rc))
            continue;
        cls[0] = 0;
        GetClassNameW(other, cls, 64);
        if (!is_static_class(cls))
            continue;
        if (rc.right <= mine.left + 4 && rc.bottom > mine.top && rc.top < mine.bottom && mine.left - rc.right < 220)
            distance = mine.left - rc.right; /* to the left, on the same row */
        else if (rc.bottom <= mine.top + 2 && mine.top - rc.bottom < 28 && rc.left < mine.right && rc.right > mine.left)
            distance = 300 + (mine.top - rc.bottom); /* above */
        if (distance >= 0 && distance < best_distance) {
            window_text(other, text, TW_NAME);
            strip_mnemonic(text, NULL, 0);
            trim_label(text);
            if (text[0]) {
                best = other;
                best_distance = distance;
                tw_copy(out, cap, text);
            }
        }
    }
    return best ? "label-near" : NULL;
}

typedef struct {
    tw_nodes *ns;
    HWND top;
    HWND focus;
    tw_remote remote;
    int remote_tried;
    int wow64;
} tw_walk;

static tw_remote *walk_remote(tw_walk *w, HWND hwnd)
{
    if (!w->remote_tried) {
        w->remote_tried = 1;
        remote_open(&w->remote, hwnd);
        w->wow64 = w->remote.wow64;
    }
    return &w->remote;
}

static void label_node(tw_node *node, HWND hwnd)
{
    const char *from = find_label(hwnd, node->name, TW_NAME);

    if (from)
        node->name_from = from;
}

static void walk_window(tw_walk *w, HWND hwnd, const tw_node *parent, int depth)
{
    tw_node *node;
    LONG style, exstyle;
    WCHAR text[TW_TEXT];
    const WCHAR *c;
    HWND child;
    int is_top = hwnd == w->top;
    LRESULT code = 0;
    RECT rc;

    if (!is_top && !IsWindowVisible(hwnd)) {
        w->ns->hidden++;
        return;
    }
    if (!GetWindowRect(hwnd, &rc) || (!is_top && (rc.right <= rc.left || rc.bottom <= rc.top))) {
        w->ns->hidden++;
        return;
    }
    node = tw_add(w->ns);
    if (!node)
        return;
    tw_window_id(node->id, TW_ID, hwnd);
    if (parent)
        tw_copy(node->parent, TW_ID, parent->id);
    node->depth = depth;
    node->kind = 'w';
    node->hwnd = hwnd;
    node->rc = rc;
    node->has_rc = 1;
    GetClassNameW(hwnd, node->cls, 64);
    c = node->cls;
    style = GetWindowLongW(hwnd, GWL_STYLE);
    exstyle = GetWindowLongW(hwnd, GWL_EXSTYLE);
    (void)exstyle;
    if (!IsWindowEnabled(hwnd))
        node->states |= TW_ST_DISABLED;
    if (hwnd == w->focus)
        node->states |= TW_ST_FOCUSED;
    window_text(hwnd, text, TW_TEXT);

    if (is_top) {
        node->role = same(c, L"#32770") ? TW_ROLE_DIALOG : TW_ROLE_FRAME;
        if (same(c, L"PuTTY") || same(c, L"ConsoleWindowClass"))
            node->role = TW_ROLE_TERMINAL;
        tw_copy(node->name, TW_NAME, text);
        node->name_from = "text";
    } else {
        code = smsg(hwnd, WM_GETDLGCODE, 0, 0);
        if (same(c, L"ComboLBox")) {
            node->role = TW_ROLE_LIST;
        } else if (contains(c, L"ComboBoxEx")) {
            node->role = TW_ROLE_PANEL;
        } else if (contains(c, L"ComboBox")) {
            node->role = TW_ROLE_COMBO_BOX;
            node->actions = TW_ACT_EXPAND | TW_ACT_COLLAPSE | TW_ACT_FOCUS;
            node->states |= TW_ST_EXPANDABLE;
            if (smsg(hwnd, CB_GETDROPPEDSTATE, 0, 0))
                node->states |= TW_ST_EXPANDED;
            if (parent && contains(parent->cls, L"ComboBoxEx") && parent->name[0]) {
                tw_copy(node->name, TW_NAME, parent->name);
                node->name_from = parent->name_from;
            } else {
                label_node(node, parent && contains(parent->cls, L"ComboBoxEx") ? parent->hwnd : hwnd);
            }
            if (text[0]) {
                tw_copy(node->text, TW_TEXT, text);
                node->has_text = 1;
            }
            items_combo(w->ns, node, style);
        } else if (contains(c, L"ListBox")) {
            node->role = TW_ROLE_LIST;
            node->actions = TW_ACT_FOCUS;
            label_node(node, hwnd);
            items_listbox(w->ns, node, style);
        } else if (contains(c, L"ListView")) {
            node->role = TW_ROLE_LIST;
            node->actions = TW_ACT_FOCUS;
            label_node(node, hwnd);
            items_listview(w->ns, node, walk_remote(w, hwnd));
        } else if (contains(c, L"TreeView")) {
            int budget = 150;
            tw_remote *r = walk_remote(w, hwnd);

            node->role = TW_ROLE_TREE;
            node->actions = TW_ACT_FOCUS;
            label_node(node, hwnd);
            if (r->ok)
                tree_items(w->ns, node, r, (HTREEITEM)smsg(hwnd, TVM_GETNEXTITEM, TVGN_ROOT, 0), node->id,
                           depth + 1, &budget);
        } else if (contains(c, L"TabControl") || contains(c, L"PageControl")) {
            node->role = TW_ROLE_PAGE_TAB_LIST;
            items_tab(w->ns, node, walk_remote(w, hwnd));
        } else if (contains(c, L"Toolbar")) {
            node->role = TW_ROLE_TOOL_BAR;
            items_toolbar(w->ns, node, walk_remote(w, hwnd));
        } else if (contains(c, L"statusbar")) {
            node->role = TW_ROLE_STATUS_BAR;
            tw_copy(node->name, TW_NAME, text);
            node->name_from = "text";
            items_status(w->ns, node, walk_remote(w, hwnd));
        } else if (same(c, L"SysHeader32")) {
            node->role = TW_ROLE_PANEL;
            items_header(w->ns, node, walk_remote(w, hwnd));
        } else if (same(c, L"msctls_trackbar32")) {
            node->role = TW_ROLE_SLIDER;
            label_node(node, hwnd);
        } else if (same(c, L"msctls_updown32")) {
            node->role = TW_ROLE_SPIN_BUTTON;
        } else if (same(c, L"msctls_progress32")) {
            node->role = TW_ROLE_PROGRESS_BAR;
        } else if (same(c, L"ScrollBar")) {
            node->role = TW_ROLE_SCROLL_BAR;
        } else if (same(c, L"SysLink")) {
            node->role = TW_ROLE_LINK;
            tw_copy(node->name, TW_NAME, text);
            node->name_from = "text";
            node->actions = TW_ACT_PRESS;
        } else if (same(c, L"tooltips_class32")) {
            node->role = TW_ROLE_TOOL_TIP;
        } else if (same(c, L"ReBarWindow32")) {
            node->role = TW_ROLE_PANEL;
        } else if (same(c, L"Scintilla") || contains(c, L"RichEdit")) {
            node->role = TW_ROLE_TEXT;
            node->states |= TW_ST_EDITABLE | TW_ST_MULTILINE;
            node->actions = TW_ACT_TYPE | TW_ACT_FOCUS;
            if (text[0]) {
                tw_copy(node->text, TW_TEXT, text);
                node->has_text = 1;
            }
            label_node(node, hwnd);
        } else if (same(c, L"Button") || contains(c, L"Button") || contains(c, L"CheckBox") ||
                   (code & (DLGC_BUTTON | DLGC_DEFPUSHBUTTON | DLGC_UNDEFPUSHBUTTON | DLGC_RADIOBUTTON))) {
            LONG type = style & 0xf;

            strip_mnemonic(text, NULL, 0);
            tw_copy(node->name, TW_NAME, text);
            node->name_from = "text";
            if (type == BS_GROUPBOX) {
                node->role = TW_ROLE_PANEL;
            } else if (type == BS_CHECKBOX || type == BS_AUTOCHECKBOX || type == BS_3STATE || type == BS_AUTO3STATE) {
                node->role = (style & BS_PUSHLIKE) ? TW_ROLE_TOGGLE_BUTTON : TW_ROLE_CHECK_BOX;
                node->actions = TW_ACT_PRESS | TW_ACT_FOCUS;
                if (smsg(hwnd, BM_GETCHECK, 0, 0) == BST_CHECKED)
                    node->states |= TW_ST_CHECKED;
            } else if (type == BS_RADIOBUTTON || type == BS_AUTORADIOBUTTON) {
                node->role = TW_ROLE_RADIO_BUTTON;
                node->actions = TW_ACT_PRESS | TW_ACT_SELECT | TW_ACT_FOCUS;
                if (smsg(hwnd, BM_GETCHECK, 0, 0) == BST_CHECKED)
                    node->states |= TW_ST_CHECKED;
            } else {
                node->role = TW_ROLE_PUSH_BUTTON;
                node->actions = TW_ACT_PRESS | TW_ACT_FOCUS;
                if (type == BS_DEFPUSHBUTTON || (code & DLGC_DEFPUSHBUTTON))
                    node->states |= TW_ST_DEFAULT;
            }
        } else if (is_edit_class(c) || (code & DLGC_HASSETSEL)) {
            DWORD from = 0, to = 0;

            node->role = (style & ES_PASSWORD) ? TW_ROLE_PASSWORD_TEXT : TW_ROLE_TEXT;
            node->actions = TW_ACT_FOCUS;
            if (style & ES_READONLY) {
                node->states |= TW_ST_READONLY;
            } else {
                node->states |= TW_ST_EDITABLE;
                node->actions |= TW_ACT_TYPE;
            }
            if (style & ES_MULTILINE)
                node->states |= TW_ST_MULTILINE;
            if (node->role == TW_ROLE_PASSWORD_TEXT) {
                node->text_withheld = 1;
            } else if (text[0]) {
                tw_copy(node->text, TW_TEXT, text);
                node->has_text = 1;
            }
            smsg(hwnd, EM_GETSEL, (WPARAM)&from, (LPARAM)&to);
            node->sel_start = (int)from;
            node->sel_end = (int)to;
            if (parent && contains(parent->cls, L"ComboBox") && parent->name[0]) {
                tw_copy(node->name, TW_NAME, parent->name);
                node->name_from = parent->name_from;
            } else {
                label_node(node, hwnd);
            }
        } else if (is_static_class(c) || (code & DLGC_STATIC)) {
            LONG type = style & 0x1f;

            strip_mnemonic(text, NULL, 0);
            tw_copy(node->name, TW_NAME, text);
            node->name_from = "text";
            if (type == SS_ICON || type == SS_BITMAP || type == SS_ENHMETAFILE)
                node->role = TW_ROLE_IMAGE;
            else if ((type >= SS_BLACKRECT && type <= SS_WHITEFRAME) || (type >= SS_ETCHEDHORZ && type <= SS_ETCHEDFRAME))
                node->role = TW_ROLE_SEPARATOR;
            else
                node->role = TW_ROLE_LABEL;
        } else {
            node->role = GetWindow(hwnd, GW_CHILD) ? TW_ROLE_PANEL : TW_ROLE_UNKNOWN;
            tw_copy(node->name, TW_NAME, text);
            if (text[0])
                node->name_from = "text";
        }
    }

    for (child = GetWindow(hwnd, GW_CHILD); child; child = GetWindow(child, GW_HWNDNEXT))
        walk_window(w, child, node, depth + 1);
}

/* ------------------------------------------- the menu, read in-process */

static WCHAR hook_path[MAX_PATH];

static void menu_file(WCHAR *path, DWORD token)
{
    WCHAR temp[MAX_PATH];
    DWORD n = GetTempPathW(MAX_PATH, temp);

    if (!n || n >= MAX_PATH)
        tw_copy(temp, MAX_PATH, L"C:\\");
    tw_copy(path, MAX_PATH, temp);
    tw_cat(path, MAX_PATH, L"tur-winbridge-menu-");
    tw_cat_hex(path, MAX_PATH, token);
    tw_cat(path, MAX_PATH, L".txt");
}

/* Reads the window's menu through tur-winhook.dll, loaded into the
 * window's own thread by a WH_CALLWNDPROC hook for the length of one
 * message. Adds a menu bar node and its items. */
static void read_menu(tw_walk *w, const tw_node *top_node, tw_buf *diag)
{
    HWND top = w->top;
    HMENU menu = GetMenu(top);
    HMODULE module;
    HOOKPROC proc;
    HHOOK hook;
    DWORD tid, token, size, got = 0;
    DWORD_PTR result = 0;
    UINT message;
    WCHAR path[MAX_PATH], *data, *line;
    HANDLE file;
    tw_node *bar;
    WCHAR ids[8][TW_ID];
    int items = 0;

    tw_printf(diag, ",\"menu_handle\":%s", menu ? "true" : "false");
    if (!menu)
        return;
    /* What another process gets from the menu functions themselves. */
    SetLastError(0);
    tw_printf(diag, ",\"menu_direct_count\":%d", GetMenuItemCount(menu));
    tw_printf(diag, ",\"menu_direct_error\":%lu", (unsigned long)GetLastError());

    if (w->wow64 || (walk_remote(w, top), w->wow64)) {
        tw_puts(diag, ",\"menu_hook\":\"not tried: a 32-bit application needs a 32-bit hook library\"");
        return;
    }
    module = LoadLibraryW(hook_path);
    if (!module) {
        tw_printf(diag, ",\"menu_hook\":\"tur-winhook.dll did not load: error %lu\"", (unsigned long)GetLastError());
        return;
    }
    proc = (HOOKPROC)GetProcAddress(module, "TwHookProc");
    message = RegisterWindowMessageW(L"TUR_WINBRIDGE_MENU_DUMP");
    tid = GetWindowThreadProcessId(top, NULL);
    if (!proc || !message || !tid) {
        tw_puts(diag, ",\"menu_hook\":\"no hook procedure, message or thread\"");
        return;
    }
    token = GetTickCount() ^ (GetCurrentProcessId() << 16) ^ (DWORD)(ULONG_PTR)top;
    menu_file(path, token);
    DeleteFileW(path);
    hook = SetWindowsHookExW(WH_CALLWNDPROC, proc, module, tid);
    if (!hook) {
        tw_printf(diag, ",\"menu_hook\":\"SetWindowsHookEx failed: error %lu\"", (unsigned long)GetLastError());
        return;
    }
    SendMessageTimeoutW(top, message, (WPARAM)token, 0, SMTO_ABORTIFHUNG, 4000, &result);
    UnhookWindowsHookEx(hook);
    file = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, 0, NULL);
    if (file == INVALID_HANDLE_VALUE) {
        tw_printf(diag, ",\"menu_hook\":\"the hook wrote nothing (message result %lu, error %lu)\"",
                  (unsigned long)result, (unsigned long)GetLastError());
        return;
    }
    size = GetFileSize(file, NULL);
    if (size == INVALID_FILE_SIZE || size > 4 * 1024 * 1024) {
        CloseHandle(file);
        DeleteFileW(path);
        tw_puts(diag, ",\"menu_hook\":\"the hook's file is too large\"");
        return;
    }
    data = calloc(size / sizeof(WCHAR) + 2, sizeof(WCHAR));
    if (!data) {
        CloseHandle(file);
        return;
    }
    ReadFile(file, data, size, &got, NULL);
    CloseHandle(file);
    DeleteFileW(path);

    bar = tw_add(w->ns);
    if (!bar) {
        free(data);
        return;
    }
    tw_copy(bar->id, TW_ID, top_node->id);
    tw_cat(bar->id, TW_ID, L"/m");
    tw_copy(bar->parent, TW_ID, top_node->id);
    bar->depth = 1;
    bar->role = TW_ROLE_MENU_BAR;
    bar->kind = 'm';
    bar->hwnd = top;
    bar->states = TW_ST_EXPANDABLE;
    tw_copy(ids[0], TW_ID, bar->id);

    /* Lines: depth TAB path TAB command TAB type TAB state TAB submenu TAB
     * left TAB top TAB right TAB bottom TAB text. */
    for (line = data; line && *line;) {
        WCHAR *end = wcschr(line, L'\n'), *fields[11], *p = line;
        int i, depth;
        tw_node *node;
        UINT type, state;

        if (end)
            *end = 0;
        for (i = 0; i < 11; i++) {
            fields[i] = p;
            if (i < 10) {
                WCHAR *tab = wcschr(p, L'\t');

                if (!tab)
                    break;
                *tab = 0;
                p = tab + 1;
            }
        }
        if (i == 11) {
            depth = (int)wcstol(fields[0], NULL, 10);
            type = (UINT)wcstoul(fields[3], NULL, 10);
            state = (UINT)wcstoul(fields[4], NULL, 10);
            if (depth >= 0 && depth < 7 && !(type & MFT_SEPARATOR) && (node = tw_add(w->ns))) {
                RECT rc;

                tw_copy(node->id, TW_ID, top_node->id);
                tw_cat(node->id, TW_ID, L"/m");
                tw_cat(node->id, TW_ID, fields[1]);
                tw_copy(node->parent, TW_ID, ids[depth]);
                tw_copy(ids[depth + 1], TW_ID, node->id);
                node->depth = depth + 2;
                node->kind = 'm';
                node->hwnd = top;
                node->command = (UINT)wcstoul(fields[2], NULL, 10);
                node->name_from = "menu";
                tw_copy(node->name, TW_NAME, fields[10]);
                strip_mnemonic(node->name, node->description, TW_NAME);
                if (fields[5][0] == L'1') {
                    node->role = TW_ROLE_MENU;
                    node->states |= TW_ST_EXPANDABLE;
                } else {
                    node->role = (state & MFS_CHECKED) ? TW_ROLE_CHECK_MENU_ITEM : TW_ROLE_MENU_ITEM;
                    node->actions = TW_ACT_PRESS;
                }
                if (state & MFS_CHECKED)
                    node->states |= TW_ST_CHECKED;
                if (state & (MFS_DISABLED | MFS_GRAYED))
                    node->states |= TW_ST_DISABLED;
                rc.left = wcstol(fields[6], NULL, 10);
                rc.top = wcstol(fields[7], NULL, 10);
                rc.right = wcstol(fields[8], NULL, 10);
                rc.bottom = wcstol(fields[9], NULL, 10);
                if (depth == 0 && rc.right > rc.left && rc.bottom > rc.top) {
                    node->rc = rc;
                    node->has_rc = 1;
                } else {
                    /* An item of a menu that is not open is not on screen. */
                    node->states |= TW_ST_OFFSCREEN;
                }
                items++;
            }
        }
        line = end ? end + 1 : NULL;
    }
    free(data);
    tw_printf(diag, ",\"menu_hook\":\"ok\",\"menu_items\":%d", items);
}

/* Read a top-level window into `ns` (Win32 tier). */
static void read_win32(HWND top, tw_nodes *ns, tw_buf *diag, int with_menu)
{
    tw_walk walk;
    GUITHREADINFO info;
    DWORD tid = GetWindowThreadProcessId(top, NULL);

    memset(&walk, 0, sizeof walk);
    walk.ns = ns;
    walk.top = top;
    memset(&info, 0, sizeof info);
    info.cbSize = sizeof info;
    if (tid && GetGUIThreadInfo(tid, &info))
        walk.focus = info.hwndFocus;
    walk_window(&walk, top, NULL, 0);
    if (with_menu && ns->n > 0)
        read_menu(&walk, &ns->v[0], diag);
    if (walk.remote_tried)
        tw_printf(diag, ",\"remote_memory\":%s,\"wow64\":%s", walk.remote.ok ? "true" : "false",
                  walk.wow64 ? "true" : "false");
    remote_close(&walk.remote);
}

/* ----------------------------------------------------------------- ops */

static HWND hwnd_of_id(const WCHAR *id)
{
    ULONG_PTR value = 0;
    const WCHAR *p;

    if (!id || id[0] != 'h')
        return NULL;
    for (p = id + 1; *p && *p != '/'; p++) {
        int digit = *p < 128 ? hexval((char)*p) : -1;

        if (digit < 0)
            return NULL;
        value = (value << 4) | (ULONG_PTR)digit;
    }
    return IsWindow((HWND)value) ? (HWND)value : NULL;
}

static int request_failed;

static void fail(tw_buf *out, long id, const char *code, const char *message)
{
    request_failed = 1;
    out->n = 0;
    tw_printf(out, "{\"id\":%ld,\"ok\":false,\"error\":{\"code\":\"%s\",\"message\":\"%s\"}}", id, code, message);
}

static void exe_of(DWORD pid, WCHAR *name, size_t cap, int *wow64)
{
    HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
    WCHAR path[MAX_PATH];
    DWORD size = MAX_PATH;
    BOOL wow = FALSE;

    name[0] = 0;
    *wow64 = 0;
    if (!process)
        return;
    if (QueryFullProcessImageNameW(process, 0, path, &size)) {
        const WCHAR *base = wcsrchr(path, L'\\');

        tw_copy(name, cap, base ? base + 1 : path);
    }
    if (IsWow64Process(process, &wow) && wow)
        *wow64 = 1;
    CloseHandle(process);
}

typedef struct {
    tw_buf *out;
    int count;
} tw_listing;

static BOOL CALLBACK list_window(HWND hwnd, LPARAM lparam)
{
    tw_listing *listing = (tw_listing *)lparam;
    tw_buf *out = listing->out;
    WCHAR text[TW_NAME], cls[64], id[TW_ID], exe[MAX_PATH];
    RECT rc;
    DWORD pid = 0;
    int wow64 = 0;
    HWND owner;

    if (!IsWindowVisible(hwnd) || !GetWindowRect(hwnd, &rc) || rc.right <= rc.left || rc.bottom <= rc.top)
        return TRUE;
    cls[0] = 0;
    GetClassNameW(hwnd, cls, 64);
    window_text(hwnd, text, TW_NAME);
    GetWindowThreadProcessId(hwnd, &pid);
    exe_of(pid, exe, MAX_PATH, &wow64);
    tw_window_id(id, TW_ID, hwnd);
    if (listing->count++)
        tw_putc(out, ',');
    tw_puts(out, "{\"window\":");
    tw_jstr(out, id);
    tw_puts(out, ",\"title\":");
    tw_jstr(out, text);
    tw_puts(out, ",\"class\":");
    tw_jstr(out, cls);
    tw_puts(out, ",\"exe\":");
    tw_jstr(out, exe);
    tw_printf(out, ",\"pid\":%lu,\"wow64\":%s,\"active\":%s,\"enabled\":%s", (unsigned long)pid,
              wow64 ? "true" : "false", GetForegroundWindow() == hwnd ? "true" : "false",
              IsWindowEnabled(hwnd) ? "true" : "false");
    owner = GetWindow(hwnd, GW_OWNER);
    if (owner) {
        tw_window_id(id, TW_ID, owner);
        tw_puts(out, ",\"owner\":");
        tw_jstr(out, id);
    }
    tw_printf(out, ",\"screen\":[%ld,%ld,%ld,%ld]}", (long)rc.left, (long)rc.top, (long)(rc.right - rc.left),
              (long)(rc.bottom - rc.top));
    return TRUE;
}

static void put_elements(tw_buf *out, tw_nodes *ns)
{
    int i;
    const RECT *top = ns->n ? &ns->v[0].rc : NULL;

    tw_puts(out, "\"elements\":[");
    for (i = 0; i < ns->n; i++) {
        if (i)
            tw_putc(out, ',');
        put_node(out, &ns->v[i], top);
    }
    tw_printf(out, "],\"visited\":%d,\"hidden\":%d,\"truncated\":%s", ns->visited, ns->hidden,
              ns->truncated ? "true" : "false");
}

static void op_read(tw_buf *out, const tw_field *fields, int count)
{
    HWND top = hwnd_of_id(field_text(fields, count, "window"));
    const WCHAR *tier = field_text(fields, count, "tier");
    tw_nodes ns;
    tw_buf diag;
    WCHAR title[TW_NAME], id[TW_ID];
    DWORD started = GetTickCount();

    if (!top) {
        tw_puts(out, "\"error\":\"no such window\"");
        return;
    }
    if (!tier)
        tier = L"win32";
    if (!nodes_init(&ns, (int)field_int(fields, count, "max_nodes", 1500))) {
        tw_puts(out, "\"error\":\"out of memory\"");
        return;
    }
    memset(&diag, 0, sizeof diag);
    tw_puts(&diag, "");
    if (same(tier, L"msaa"))
        tw_msaa_read(top, &ns, &diag);
    else if (same(tier, L"uia"))
        tw_uia_read(top, &ns, &diag);
    else
        read_win32(top, &ns, &diag, field_int(fields, count, "menu", 1) != 0);
    window_text(top, title, TW_NAME);
    tw_window_id(id, TW_ID, top);
    tw_puts(out, "\"window\":");
    tw_jstr(out, id);
    tw_puts(out, ",\"title\":");
    tw_jstr(out, title);
    tw_puts(out, ",\"tier\":");
    tw_jstr(out, tier);
    tw_printf(out, ",\"milliseconds\":%lu,", (unsigned long)(GetTickCount() - started));
    put_elements(out, &ns);
    if (diag.p)
        tw_puts(out, diag.p);
    free(diag.p);
    nodes_free(&ns);
}

/* Whether `text` is in what the control holds now. */
static int holds_text(HWND hwnd, const WCHAR *text)
{
    static WCHAR buffer[8192];
    size_t i, j;

    buffer[0] = 0;
    smsg(hwnd, WM_GETTEXT, 8191, (LPARAM)buffer);
    buffer[8191] = 0;
    for (i = 0; buffer[i]; i++) {
        for (j = 0; text[j] && buffer[i + j] == text[j]; j++)
            ;
        if (!text[j])
            return 1;
    }
    return 0;
}

static void notify_parent(HWND control, UINT code)
{
    HWND parent = GetParent(control);

    if (parent)
        PostMessageW(parent, WM_COMMAND, MAKEWPARAM(GetDlgCtrlID(control), code), (LPARAM)control);
}

static void act_win32(tw_buf *out, tw_node *node, const WCHAR *action, const WCHAR *text, const WCHAR *how)
{
    HWND h = node->hwnd, root = GetAncestor(h, GA_ROOT);
    const char *did = NULL;
    int pressable = node->role == TW_ROLE_PUSH_BUTTON || node->role == TW_ROLE_CHECK_BOX ||
                    node->role == TW_ROLE_RADIO_BUTTON || node->role == TW_ROLE_TOGGLE_BUTTON ||
                    node->role == TW_ROLE_LINK;

    if (!how)
        how = L"auto";
    if (node->states & TW_ST_DISABLED) {
        tw_puts(out, "\"done\":false,\"code\":\"failed\",\"why\":\"the control is disabled\"");
        return;
    }
    if (same(action, L"press") || (same(action, L"select") && node->kind == 'w' && node->role == TW_ROLE_RADIO_BUTTON)) {
        if (node->kind == 'm') {
            if (node->states & TW_ST_EXPANDABLE) {
                tw_puts(out, "\"done\":false,\"code\":\"not_offered\",\"why\":\"a menu with items is expanded, not pressed\"");
                return;
            }
            PostMessageW(h, WM_COMMAND, MAKEWPARAM(node->command, 0), 0);
            did = "WM_COMMAND with the menu item's identifier, posted to the window";
        } else if (node->kind == 'b') {
            PostMessageW(GetParent(h), WM_COMMAND, MAKEWPARAM(node->command, 0), (LPARAM)h);
            did = "WM_COMMAND with the toolbar button's command, posted to the toolbar's parent";
        } else if (node->kind == 'w' && pressable) {
            if (same(how, L"command") || (same(how, L"auto") && node->role == TW_ROLE_PUSH_BUTTON)) {
                notify_parent(h, BN_CLICKED);
                did = "WM_COMMAND BN_CLICKED posted to the control's parent";
            } else {
                SetForegroundWindow(root);
                PostMessageW(h, BM_CLICK, 0, 0);
                did = "BM_CLICK posted to the control";
            }
        }
    } else if (same(action, L"type")) {
        if (node->kind == 'w' && (node->role == TW_ROLE_TEXT || node->role == TW_ROLE_PASSWORD_TEXT) && text &&
            text[0]) {
            if ((is_edit_class(node->cls) || contains(node->cls, L"RichEdit")) && !same(how, L"char")) {
                smsg(h, EM_REPLACESEL, TRUE, (LPARAM)text);
                did = "EM_REPLACESEL at the caret";
            } else {
                const WCHAR *p;

                for (p = text; *p; p++)
                    PostMessageW(h, WM_CHAR, (WPARAM)*p, 1);
                Sleep(150 + 3 * (DWORD)lstrlenW(text));
                did = "WM_CHAR posted for each character";
            }
            tw_printf(out, "\"done\":true,\"how\":\"%s\",\"text_now_contains_typed_text\":%s", did,
                      node->role == TW_ROLE_PASSWORD_TEXT ? "null" : (holds_text(h, text) ? "true" : "false"));
            return;
        }
    } else if (same(action, L"select")) {
        if (node->kind == 'i' && node->ctl == 'L') {
            smsg(h, LB_SETCURSEL, (WPARAM)node->item, 0);
            notify_parent(h, LBN_SELCHANGE);
            did = "LB_SETCURSEL, then LBN_SELCHANGE posted to the parent";
        } else if (node->kind == 'i' && node->ctl == 'C') {
            smsg(h, CB_SETCURSEL, (WPARAM)node->item, 0);
            notify_parent(h, CBN_SELCHANGE);
            did = "CB_SETCURSEL, then CBN_SELCHANGE posted to the parent";
        } else if (node->kind == 'i' && node->ctl == 'T') {
            smsg(h, TCM_SETCURFOCUS, (WPARAM)node->item, 0);
            did = "TCM_SETCURFOCUS";
        } else if (node->kind == 'i' && node->ctl == 'V') {
            tw_remote r;

            if (remote_open(&r, h)) {
                LVITEMW item;

                memset(&item, 0, sizeof item);
                item.stateMask = LVIS_SELECTED | LVIS_FOCUSED;
                item.state = 0;
                if (remote_put(&r, &item, sizeof item))
                    smsg(h, LVM_SETITEMSTATE, (WPARAM)-1, (LPARAM)r.memory);
                item.state = LVIS_SELECTED | LVIS_FOCUSED;
                if (remote_put(&r, &item, sizeof item) && smsg(h, LVM_SETITEMSTATE, (WPARAM)node->item, (LPARAM)r.memory))
                    did = "LVM_SETITEMSTATE through memory in the application";
                remote_close(&r);
            }
        } else if (node->kind == 't') {
            if (smsg(h, TVM_SELECTITEM, TVGN_CARET, (LPARAM)node->handle))
                did = "TVM_SELECTITEM";
        }
    } else if (same(action, L"expand") || same(action, L"collapse")) {
        int expand = same(action, L"expand");

        if (node->kind == 't') {
            smsg(h, TVM_EXPAND, expand ? TVE_EXPAND : TVE_COLLAPSE, (LPARAM)node->handle);
            did = "TVM_EXPAND";
        } else if (node->kind == 'w' && node->role == TW_ROLE_COMBO_BOX) {
            smsg(h, CB_SHOWDROPDOWN, (WPARAM)expand, 0);
            did = "CB_SHOWDROPDOWN";
        }
    } else if (same(action, L"focus")) {
        if (node->kind == 'w') {
            WCHAR cls[64];
            HWND parent = GetParent(h);

            cls[0] = 0;
            if (parent)
                GetClassNameW(parent, cls, 64);
            if (parent && same(cls, L"#32770")) {
                PostMessageW(parent, WM_NEXTDLGCTL, (WPARAM)h, TRUE);
                did = "WM_NEXTDLGCTL posted to the dialog";
            } else {
                DWORD tid = GetWindowThreadProcessId(h, NULL);

                AttachThreadInput(GetCurrentThreadId(), tid, TRUE);
                SetFocus(h);
                AttachThreadInput(GetCurrentThreadId(), tid, FALSE);
                did = "SetFocus with the threads' input attached";
            }
        }
    } else if (same(action, L"scroll")) {
        if (node->kind == 'i' && node->ctl == 'L') {
            smsg(h, LB_SETTOPINDEX, (WPARAM)node->item, 0);
            did = "LB_SETTOPINDEX";
        } else if (node->kind == 'i' && node->ctl == 'V') {
            smsg(h, LVM_ENSUREVISIBLE, (WPARAM)node->item, FALSE);
            did = "LVM_ENSUREVISIBLE";
        } else if (node->kind == 't') {
            smsg(h, TVM_ENSUREVISIBLE, 0, (LPARAM)node->handle);
            did = "TVM_ENSUREVISIBLE";
        }
    }
    if (did)
        tw_printf(out, "\"done\":true,\"how\":\"%s\"", did);
    else
        tw_puts(out, "\"done\":false,\"code\":\"not_offered\",\"why\":\"this element does not offer that through the Win32 tier\"");
}

/* Read the top-level window of an element's identifier (Win32 tier) and
 * find the element. The caller frees `ns`. */
static tw_node *resolve_element(const WCHAR *id, tw_nodes *ns, tw_buf *diag)
{
    HWND hwnd = hwnd_of_id(id), top;

    if (!hwnd)
        return NULL;
    top = GetAncestor(hwnd, GA_ROOT);
    if (!top || !nodes_init(ns, 4000))
        return NULL;
    read_win32(top, ns, diag, wcschr(id, L'/') && wcsstr(id, L"/m") != NULL);
    return find_node(ns, id);
}

static void op_describe(tw_buf *out, const tw_field *fields, int count)
{
    const WCHAR *id = field_text(fields, count, "element");
    tw_nodes ns;
    tw_buf diag;
    tw_node *node;

    memset(&ns, 0, sizeof ns);
    memset(&diag, 0, sizeof diag);
    node = id ? resolve_element(id, &ns, &diag) : NULL;
    if (!node) {
        tw_puts(out, "\"found\":false");
    } else {
        tw_puts(out, "\"found\":true,\"element\":");
        put_node(out, node, ns.n ? &ns.v[0].rc : NULL);
        tw_puts(out, ",\"window\":");
        put_node(out, &ns.v[0], NULL);
    }
    free(diag.p);
    nodes_free(&ns);
}

static void op_act(tw_buf *out, const tw_field *fields, int count)
{
    const WCHAR *id = field_text(fields, count, "element");
    const WCHAR *action = field_text(fields, count, "action");
    const WCHAR *tier = field_text(fields, count, "tier");
    const WCHAR *text = field_text(fields, count, "text");
    tw_nodes ns;
    tw_buf diag;
    tw_node *node;

    if (!id || !action) {
        tw_puts(out, "\"done\":false,\"code\":\"failed\",\"why\":\"no element or action\"");
        return;
    }
    if (tier && same(tier, L"msaa")) {
        HWND hwnd = hwnd_of_id(id);

        if (hwnd)
            tw_msaa_act(hwnd, action, text, out);
        else
            tw_puts(out, "\"done\":false,\"code\":\"not_found\",\"why\":\"no such window\"");
        return;
    }
    if (tier && same(tier, L"uia")) {
        HWND hwnd = hwnd_of_id(id);

        if (hwnd)
            tw_uia_act(hwnd, action, out);
        else
            tw_puts(out, "\"done\":false,\"code\":\"not_found\",\"why\":\"no such window\"");
        return;
    }
    memset(&ns, 0, sizeof ns);
    memset(&diag, 0, sizeof diag);
    node = resolve_element(id, &ns, &diag);
    if (!node)
        tw_puts(out, "\"done\":false,\"code\":\"not_found\",\"why\":\"the element is not there\"");
    else
        act_win32(out, node, action, text, field_text(fields, count, "how"));
    free(diag.p);
    nodes_free(&ns);
}

/* The element of a window under a point of the screen, and the window
 * that is really on top there. */
static void op_at_point(tw_buf *out, const tw_field *fields, int count)
{
    HWND top = hwnd_of_id(field_text(fields, count, "window")), seen, seen_root;
    POINT point;
    tw_nodes ns;
    tw_buf diag;
    tw_node *best = NULL;
    long best_area = 0;
    int i;
    WCHAR id[TW_ID], cls[64];

    point.x = field_int(fields, count, "x", -1);
    point.y = field_int(fields, count, "y", -1);
    if (!top || !nodes_init(&ns, 1500)) {
        tw_puts(out, "\"found\":false,\"why\":\"no such window\"");
        return;
    }
    memset(&diag, 0, sizeof diag);
    read_win32(top, &ns, &diag, 1);
    for (i = 1; i < ns.n; i++) {
        tw_node *node = &ns.v[i];
        long area;

        if (!node->has_rc || (node->states & TW_ST_OFFSCREEN) || !PtInRect(&node->rc, point))
            continue;
        area = (long)(node->rc.right - node->rc.left) * (long)(node->rc.bottom - node->rc.top);
        if (!best || node->depth > best->depth || (node->depth == best->depth && area < best_area)) {
            best = node;
            best_area = area;
        }
    }
    seen = WindowFromPoint(point);
    seen_root = seen ? GetAncestor(seen, GA_ROOT) : NULL;
    tw_printf(out, "\"found\":%s", best ? "true" : "false");
    if (best) {
        tw_puts(out, ",\"element\":");
        put_node(out, best, &ns.v[0].rc);
    }
    tw_printf(out, ",\"in_window\":%s", ns.n && PtInRect(&ns.v[0].rc, point) ? "true" : "false");
    tw_puts(out, ",\"topmost\":");
    if (seen) {
        cls[0] = 0;
        GetClassNameW(seen, cls, 64);
        tw_window_id(id, TW_ID, seen);
        tw_puts(out, "{\"id\":");
        tw_jstr(out, id);
        tw_window_id(id, TW_ID, seen_root);
        tw_puts(out, ",\"root\":");
        tw_jstr(out, id);
        tw_puts(out, ",\"class\":");
        tw_jstr(out, cls);
        tw_printf(out, ",\"same_window\":%s}", seen_root == top ? "true" : "false");
    } else {
        tw_puts(out, "null");
    }
    free(diag.p);
    nodes_free(&ns);
}

/* A click at a point of a window, by position alone: what acting on
 * coordinates without asking what is there comes to. For the experiment's
 * attack only. */
static void op_click_point(tw_buf *out, const tw_field *fields, int count)
{
    HWND top = hwnd_of_id(field_text(fields, count, "window")), target, next;
    POINT screen, client;
    WCHAR id[TW_ID];
    int guard = 0;

    screen.x = field_int(fields, count, "x", -1);
    screen.y = field_int(fields, count, "y", -1);
    if (!top) {
        tw_puts(out, "\"done\":false");
        return;
    }
    target = top;
    for (;;) {
        client = screen;
        ScreenToClient(target, &client);
        next = ChildWindowFromPointEx(target, client, CWP_SKIPINVISIBLE | CWP_SKIPTRANSPARENT);
        if (!next || next == target || ++guard > 30)
            break;
        target = next;
    }
    client = screen;
    ScreenToClient(target, &client);
    PostMessageW(target, WM_LBUTTONDOWN, MK_LBUTTON, MAKELPARAM(client.x, client.y));
    PostMessageW(target, WM_LBUTTONUP, 0, MAKELPARAM(client.x, client.y));
    tw_window_id(id, TW_ID, target);
    tw_puts(out, "\"done\":true,\"target\":");
    tw_jstr(out, id);
}

static void op_hello(tw_buf *out)
{
    HMODULE ntdll = GetModuleHandleW(L"ntdll.dll");
    const char *(CDECL *version)(void) = NULL;
    HMODULE hook;

    if (ntdll)
        version = (const char *(CDECL *)(void))GetProcAddress(ntdll, "wine_get_version");
    tw_printf(out, "\"bridge\":\"tur-winbridge\",\"protocol\":1,\"pid\":%lu,\"bits\":%d",
              (unsigned long)GetCurrentProcessId(), (int)(8 * sizeof(void *)));
    tw_printf(out, ",\"wine\":\"%s\"", version ? version() : "");
    hook = LoadLibraryW(hook_path);
    tw_printf(out, ",\"hook_library\":%s", hook ? "true" : "false");
}

int main(void)
{
    static char line[262144];
    tw_buf out;
    WCHAR *slash;

    memset(&out, 0, sizeof out);
    _setmode(_fileno(stdout), _O_BINARY);
    _setmode(_fileno(stdin), _O_BINARY);
    CoInitializeEx(NULL, COINIT_APARTMENTTHREADED);
    GetModuleFileNameW(NULL, hook_path, MAX_PATH);
    slash = wcsrchr(hook_path, L'\\');
    if (slash)
        slash[1] = 0;
    else
        hook_path[0] = 0;
    tw_cat(hook_path, MAX_PATH, L"tur-winhook.dll");

    while (fgets(line, sizeof line, stdin)) {
        tw_field fields[TW_FIELDS];
        int count, i, quit = 0;
        long id;
        const WCHAR *op;

        memset(fields, 0, sizeof fields);
        count = parse_request(line, fields, TW_FIELDS);
        request_failed = 0;
        out.n = 0;
        tw_puts(&out, "");
        if (count < 0) {
            fail(&out, 0, "failed", "the request is not a JSON object");
        } else {
            id = field_int(fields, count, "id", 0);
            op = field_text(fields, count, "op");
            tw_printf(&out, "{\"id\":%ld,\"ok\":true,\"result\":{", id);
            if (!op) {
                fail(&out, id, "failed", "no op");
            } else if (same(op, L"hello")) {
                op_hello(&out);
            } else if (same(op, L"windows")) {
                tw_listing listing;

                listing.out = &out;
                listing.count = 0;
                tw_puts(&out, "\"windows\":[");
                EnumWindows(list_window, (LPARAM)&listing);
                tw_putc(&out, ']');
            } else if (same(op, L"read")) {
                op_read(&out, fields, count);
            } else if (same(op, L"describe")) {
                op_describe(&out, fields, count);
            } else if (same(op, L"act")) {
                op_act(&out, fields, count);
            } else if (same(op, L"at_point")) {
                op_at_point(&out, fields, count);
            } else if (same(op, L"click_point")) {
                op_click_point(&out, fields, count);
            } else if (same(op, L"close")) {
                HWND hwnd = hwnd_of_id(field_text(fields, count, "window"));

                if (hwnd)
                    PostMessageW(hwnd, WM_CLOSE, 0, 0);
                tw_printf(&out, "\"done\":%s", hwnd ? "true" : "false");
            } else if (same(op, L"foreground")) {
                HWND hwnd = hwnd_of_id(field_text(fields, count, "window"));

                tw_printf(&out, "\"done\":%s", hwnd && SetForegroundWindow(hwnd) ? "true" : "false");
            } else if (same(op, L"com_list")) {
                tw_com_list((int)field_int(fields, count, "max", 20000), &out);
            } else if (same(op, L"com_typeinfo")) {
                tw_com_typeinfo(field_text(fields, count, "progid"), &out);
            } else if (same(op, L"com_invoke")) {
                const WCHAR *args[4];
                int nargs = 0;

                for (i = 0; i < 4; i++) {
                    char key[8];

                    snprintf(key, sizeof key, "arg%d", i);
                    args[i] = field_text(fields, count, key);
                    if (args[i])
                        nargs = i + 1;
                }
                tw_com_invoke(field_text(fields, count, "progid"), field_text(fields, count, "member"),
                              field_text(fields, count, "kind"), args, nargs, &out);
            } else if (same(op, L"quit")) {
                tw_puts(&out, "\"bye\":true");
                quit = 1;
            } else {
                fail(&out, id, "failed", "no such op");
            }
            if (!request_failed)
                tw_puts(&out, "}}");
        }
        tw_putc(&out, '\n');
        fwrite(out.p, 1, out.n, stdout);
        fflush(stdout);
        for (i = 0; i < count && i < TW_FIELDS; i++)
            free(fields[i].value);
        if (quit)
            break;
    }
    return 0;
}
