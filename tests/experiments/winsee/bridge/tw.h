/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * tur-winbridge: the experiment's bridge inside a Wine prefix
 * (docs/reports/exp-winsee-1.md). Shared declarations of its parts:
 *
 *   tw_core.c   the protocol on standard input and output, the Win32 tier
 *               (window tree, classes, text, styles, rectangles, standard
 *               control messages), acting, the point test
 *   tw_msaa.c   the MSAA tier (oleacc, IAccessible)
 *   tw_uia.c    the UI Automation tier (uiautomationcore's COM client)
 *   tw_com.c    COM automation: registered objects, type libraries, calls
 *   tw_hook.c   tur-winhook.dll: reads a window's menu inside its process
 *
 * Experiment code: built and run only by .github/workflows/exp-winsee.yml.
 */
#ifndef TW_H
#define TW_H

#ifndef UNICODE
#define UNICODE
#endif
#ifndef _UNICODE
#define _UNICODE
#endif
#ifndef COBJMACROS
#define COBJMACROS
#endif

#include <windows.h>
#include <commctrl.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

/* A growing byte buffer that holds ASCII JSON. */
typedef struct {
    char *p;
    size_t n, cap;
} tw_buf;

void tw_putc(tw_buf *b, char c);
void tw_puts(tw_buf *b, const char *s);
void tw_printf(tw_buf *b, const char *fmt, ...);
void tw_hex(tw_buf *b, ULONG_PTR v);
/* A JSON string from UTF-16; everything outside printable ASCII is escaped. */
void tw_jstr(tw_buf *b, const WCHAR *w);
/* An HRESULT as "0x8000ffff". */
void tw_hr(tw_buf *b, HRESULT hr);

/* States (the names tur-pax-a11y reports) and what can be done. */
#define TW_ST_DISABLED 0x001
#define TW_ST_FOCUSED 0x002
#define TW_ST_CHECKED 0x004
#define TW_ST_SELECTED 0x008
#define TW_ST_EXPANDED 0x010
#define TW_ST_EDITABLE 0x020
#define TW_ST_DEFAULT 0x040
#define TW_ST_READONLY 0x080
#define TW_ST_OFFSCREEN 0x100
#define TW_ST_EXPANDABLE 0x200
#define TW_ST_MULTILINE 0x400

#define TW_ACT_PRESS 0x01
#define TW_ACT_TYPE 0x02
#define TW_ACT_SELECT 0x04
#define TW_ACT_EXPAND 0x08
#define TW_ACT_COLLAPSE 0x10
#define TW_ACT_FOCUS 0x20
#define TW_ACT_SCROLL 0x40

/* AT-SPI role numbers used here (atspi-constants.h), so that the engine's
 * UiRole::from_atspi classifies a Windows control as it does a Linux one. */
#define TW_ROLE_CHECK_BOX 7
#define TW_ROLE_CHECK_MENU_ITEM 8
#define TW_ROLE_COMBO_BOX 11
#define TW_ROLE_DIALOG 16
#define TW_ROLE_FRAME 23
#define TW_ROLE_IMAGE 27
#define TW_ROLE_LABEL 29
#define TW_ROLE_LIST 31
#define TW_ROLE_LIST_ITEM 32
#define TW_ROLE_MENU 33
#define TW_ROLE_MENU_BAR 34
#define TW_ROLE_MENU_ITEM 35
#define TW_ROLE_PAGE_TAB 37
#define TW_ROLE_PAGE_TAB_LIST 38
#define TW_ROLE_PANEL 39
#define TW_ROLE_PASSWORD_TEXT 40
#define TW_ROLE_PROGRESS_BAR 42
#define TW_ROLE_PUSH_BUTTON 43
#define TW_ROLE_RADIO_BUTTON 44
#define TW_ROLE_SCROLL_BAR 48
#define TW_ROLE_SEPARATOR 50
#define TW_ROLE_SLIDER 51
#define TW_ROLE_SPIN_BUTTON 52
#define TW_ROLE_STATUS_BAR 54
#define TW_ROLE_TABLE_COLUMN_HEADER 57
#define TW_ROLE_TERMINAL 60
#define TW_ROLE_TEXT 61
#define TW_ROLE_TOGGLE_BUTTON 62
#define TW_ROLE_TOOL_BAR 63
#define TW_ROLE_TOOL_TIP 64
#define TW_ROLE_TREE 65
#define TW_ROLE_UNKNOWN 67
#define TW_ROLE_WINDOW 69
#define TW_ROLE_LINK 88
#define TW_ROLE_TREE_ITEM 91
#define TW_ROLE_TITLE_BAR 119

const char *tw_role_name(int role);

#define TW_ID 96
#define TW_NAME 201
#define TW_TEXT 401

/* One element, in the node model of tur-pax-a11y, with what the experiment
 * adds: the Windows class, the screen rectangle, the native role. */
typedef struct {
    WCHAR id[TW_ID];
    WCHAR parent[TW_ID];
    int depth;
    int role;        /* AT-SPI role number */
    int native_role; /* MSAA role or UIA control type; 0 for the Win32 tier */
    WCHAR name[TW_NAME];
    WCHAR description[TW_NAME];
    WCHAR text[TW_TEXT];
    int has_text;
    int text_withheld;
    WCHAR cls[64];
    RECT rc; /* screen coordinates */
    int has_rc;
    unsigned states;
    unsigned actions;
    const char *name_from; /* "text", "label", "label-near", "item", "menu" */
    int sel_start, sel_end; /* EM_GETSEL of an edit control; -1 if none */
    /* What acting needs (Win32 tier). */
    HWND hwnd;
    int item;        /* index of a sub-item, or -1 */
    ULONG_PTR handle; /* a tree item's handle */
    UINT command;    /* a menu item's or toolbar button's command */
    char kind;       /* 'w' window or control, 'i' sub-item, 't' tree item, 'm' menu item, 'b' toolbar button */
    char ctl;        /* of a sub-item: 'L' list box, 'C' combo box, 'V' list view, 'T' tab control, 'S' status bar, 'H' header */
} tw_node;

typedef struct {
    tw_node *v;
    int n, cap;
    int max;      /* the most nodes to keep */
    int visited;  /* windows and items looked at */
    int hidden;   /* invisible windows left out with their children */
    int truncated;
} tw_nodes;

/* A new zeroed node, or NULL when the budget is used up. */
tw_node *tw_add(tw_nodes *ns);

/* The tiers that are not the Win32 tier. `diag` receives JSON members
 * (each beginning with a comma) that say what the tier answered. */
void tw_msaa_read(HWND top, tw_nodes *ns, tw_buf *diag);
void tw_msaa_act(HWND hwnd, const WCHAR *action, const WCHAR *text, tw_buf *out);
void tw_uia_read(HWND top, tw_nodes *ns, tw_buf *diag);
void tw_uia_act(HWND hwnd, const WCHAR *action, tw_buf *out);

/* COM automation. Each writes one JSON object. */
void tw_com_list(int max, tw_buf *out);
void tw_com_typeinfo(const WCHAR *progid, tw_buf *out);
void tw_com_invoke(const WCHAR *progid, const WCHAR *member, const WCHAR *kind,
                   const WCHAR **args, int nargs, tw_buf *out);

/* Helpers of tw_core.c the tiers use. */
void tw_copy(WCHAR *dst, size_t cap, const WCHAR *src);
void tw_cat(WCHAR *dst, size_t cap, const WCHAR *src);
void tw_cat_hex(WCHAR *dst, size_t cap, ULONG_PTR v);
void tw_cat_int(WCHAR *dst, size_t cap, long v);
void tw_window_id(WCHAR *dst, size_t cap, HWND hwnd);

#endif
