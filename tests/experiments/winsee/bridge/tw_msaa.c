/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * tur-winbridge, the MSAA tier: what oleacc's IAccessible gives for a
 * window and its child windows, read the way a Windows client reads it
 * (AccessibleObjectFromWindow, AccessibleChildren, the properties of each
 * object), and what its actions answer. Built with -DTW_STUB when the
 * headers at hand do not carry it; the tier then says so.
 */
#include "tw.h"

#ifndef TW_STUB

#include <oleacc.h>

static const GUID tw_IID_IAccessible = {0x618736e0, 0x3c3d, 0x11cf, {0x81, 0x0c, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71}};

static int role_from_msaa(long role)
{
    switch (role) {
    case 0x01: return TW_ROLE_TITLE_BAR;
    case 0x02: return TW_ROLE_MENU_BAR;
    case 0x03: return TW_ROLE_SCROLL_BAR;
    case 0x09: return TW_ROLE_WINDOW;
    case 0x0a: return TW_ROLE_PANEL; /* ROLE_SYSTEM_CLIENT: nothing more is known */
    case 0x0b: return TW_ROLE_MENU;
    case 0x0c: return TW_ROLE_MENU_ITEM;
    case 0x0d: return TW_ROLE_TOOL_TIP;
    case 0x12: return TW_ROLE_DIALOG;
    case 0x14: return TW_ROLE_PANEL;
    case 0x15: return TW_ROLE_SEPARATOR;
    case 0x16: return TW_ROLE_TOOL_BAR;
    case 0x17: return TW_ROLE_STATUS_BAR;
    case 0x1e: return TW_ROLE_LINK;
    case 0x21: return TW_ROLE_LIST;
    case 0x22: return TW_ROLE_LIST_ITEM;
    case 0x23: return TW_ROLE_TREE;
    case 0x24: return TW_ROLE_TREE_ITEM;
    case 0x25: return TW_ROLE_PAGE_TAB;
    case 0x28: return TW_ROLE_IMAGE;
    case 0x29: return TW_ROLE_LABEL;
    case 0x2a: return TW_ROLE_TEXT;
    case 0x2b: return TW_ROLE_PUSH_BUTTON;
    case 0x2c: return TW_ROLE_CHECK_BOX;
    case 0x2d: return TW_ROLE_RADIO_BUTTON;
    case 0x2e: return TW_ROLE_COMBO_BOX;
    case 0x30: return TW_ROLE_PROGRESS_BAR;
    case 0x33: return TW_ROLE_SLIDER;
    case 0x34: return TW_ROLE_SPIN_BUTTON;
    case 0x3c: return TW_ROLE_PAGE_TAB_LIST;
    default: return TW_ROLE_UNKNOWN;
    }
}

typedef struct {
    tw_nodes *ns;
    int objects;
    int failures;
    HRESULT first_failure;
    int named;
    int with_action;
    int simple_children;
} msaa_walk;

static void msaa_fill(msaa_walk *w, tw_node *node, IAccessible *acc, VARIANT child)
{
    BSTR text = NULL;
    VARIANT value;
    long left = 0, top = 0, width = 0, height = 0;
    HRESULT hr;

    hr = IAccessible_get_accName(acc, child, &text);
    if (hr == S_OK && text && text[0]) {
        tw_copy(node->name, TW_NAME, text);
        node->name_from = "msaa";
        w->named++;
    }
    SysFreeString(text);
    text = NULL;

    VariantInit(&value);
    hr = IAccessible_get_accRole(acc, child, &value);
    if (hr == S_OK && V_VT(&value) == VT_I4) {
        node->native_role = (int)V_I4(&value);
        node->role = role_from_msaa(V_I4(&value));
    }
    VariantClear(&value);

    VariantInit(&value);
    hr = IAccessible_get_accState(acc, child, &value);
    if (hr == S_OK && V_VT(&value) == VT_I4) {
        long state = V_I4(&value);

        if (state & 0x1)
            node->states |= TW_ST_DISABLED;
        if (state & 0x2)
            node->states |= TW_ST_SELECTED;
        if (state & 0x4)
            node->states |= TW_ST_FOCUSED;
        if (state & 0x10)
            node->states |= TW_ST_CHECKED;
        if (state & 0x40)
            node->states |= TW_ST_READONLY;
        if (state & 0x100)
            node->states |= TW_ST_DEFAULT;
        if (state & 0x200)
            node->states |= TW_ST_EXPANDED;
        if (state & (0x8000 | 0x10000))
            node->states |= TW_ST_OFFSCREEN;
        if ((state & 0x20000000) && node->role == TW_ROLE_TEXT) {
            node->role = TW_ROLE_PASSWORD_TEXT;
            node->text_withheld = 1;
        }
    }
    VariantClear(&value);

    hr = IAccessible_accLocation(acc, &left, &top, &width, &height, child);
    if (hr == S_OK && width > 0 && height > 0) {
        node->rc.left = left;
        node->rc.top = top;
        node->rc.right = left + width;
        node->rc.bottom = top + height;
        node->has_rc = 1;
    }

    if (!node->text_withheld) {
        hr = IAccessible_get_accValue(acc, child, &text);
        if (hr == S_OK && text && text[0]) {
            tw_copy(node->text, TW_TEXT, text);
            node->has_text = 1;
        }
        SysFreeString(text);
        text = NULL;
    }

    hr = IAccessible_get_accDefaultAction(acc, child, &text);
    if (hr == S_OK && text && text[0]) {
        tw_copy(node->description, TW_NAME, L"default action: ");
        tw_cat(node->description, TW_NAME, text);
        node->actions |= TW_ACT_PRESS;
        w->with_action++;
    }
    SysFreeString(text);
}

static void msaa_window(msaa_walk *w, HWND hwnd, const WCHAR *parent_id, int depth)
{
    IAccessible *acc = NULL;
    HRESULT hr;
    long count = 0;
    tw_node *node;
    VARIANT self;
    WCHAR id[TW_ID];

    if (depth > 25)
        return;
    hr = AccessibleObjectFromWindow(hwnd, (DWORD)OBJID_CLIENT, &tw_IID_IAccessible, (void **)&acc);
    if (FAILED(hr) || !acc) {
        w->failures++;
        if (!w->first_failure)
            w->first_failure = hr;
        return;
    }
    node = tw_add(w->ns);
    if (!node) {
        IAccessible_Release(acc);
        return;
    }
    w->objects++;
    tw_window_id(node->id, TW_ID, hwnd);
    tw_copy(id, TW_ID, node->id);
    if (parent_id)
        tw_copy(node->parent, TW_ID, parent_id);
    node->depth = depth;
    node->kind = 'w';
    node->hwnd = hwnd;
    GetClassNameW(hwnd, node->cls, 64);
    VariantInit(&self);
    V_VT(&self) = VT_I4;
    V_I4(&self) = CHILDID_SELF;
    msaa_fill(w, node, acc, self);

    hr = IAccessible_get_accChildCount(acc, &count);
    if (hr == S_OK && count > 0) {
        VARIANT *children;
        LONG got = 0, i;

        if (count > 400)
            count = 400;
        children = calloc((size_t)count, sizeof(VARIANT));
        if (children) {
            hr = AccessibleChildren(acc, 0, count, children, &got);
            for (i = 0; SUCCEEDED(hr) && i < got; i++) {
                if (V_VT(&children[i]) == VT_DISPATCH && V_DISPATCH(&children[i])) {
                    IAccessible *child = NULL;
                    HWND child_window = NULL;

                    if (SUCCEEDED(IDispatch_QueryInterface(V_DISPATCH(&children[i]), &tw_IID_IAccessible,
                                                           (void **)&child)) &&
                        child) {
                        if (SUCCEEDED(WindowFromAccessibleObject(child, &child_window)) && child_window &&
                            child_window != hwnd) {
                            if (IsWindowVisible(child_window))
                                msaa_window(w, child_window, id, depth + 1);
                            else
                                w->ns->hidden++;
                        } else {
                            /* An object that is not a child window: kept as it is. */
                            tw_node *extra = tw_add(w->ns);

                            if (extra) {
                                tw_copy(extra->id, TW_ID, id);
                                tw_cat(extra->id, TW_ID, L"/o");
                                tw_cat_int(extra->id, TW_ID, (long)i);
                                tw_copy(extra->parent, TW_ID, id);
                                extra->depth = depth + 1;
                                extra->kind = 'i';
                                extra->hwnd = hwnd;
                                msaa_fill(w, extra, child, self);
                                w->objects++;
                            }
                        }
                        IAccessible_Release(child);
                    }
                } else if (V_VT(&children[i]) == VT_I4) {
                    tw_node *simple = tw_add(w->ns);

                    if (simple) {
                        tw_copy(simple->id, TW_ID, id);
                        tw_cat(simple->id, TW_ID, L"/c");
                        tw_cat_int(simple->id, TW_ID, (long)V_I4(&children[i]));
                        tw_copy(simple->parent, TW_ID, id);
                        simple->depth = depth + 1;
                        simple->kind = 'i';
                        simple->hwnd = hwnd;
                        simple->item = (int)V_I4(&children[i]);
                        msaa_fill(w, simple, acc, children[i]);
                        w->simple_children++;
                    }
                }
                VariantClear(&children[i]);
            }
            free(children);
        }
    }
    IAccessible_Release(acc);
}

/* What the objects of a window other than its client area answer. */
static void probe(HWND top, LONG object, const char *name, tw_buf *diag)
{
    IAccessible *acc = NULL;
    HRESULT hr = AccessibleObjectFromWindow(top, (DWORD)object, &tw_IID_IAccessible, (void **)&acc);
    long count = -1;

    tw_printf(diag, ",\"msaa_%s\":", name);
    tw_hr(diag, hr);
    if (SUCCEEDED(hr) && acc) {
        HRESULT counted = IAccessible_get_accChildCount(acc, &count);

        tw_printf(diag, ",\"msaa_%s_children\":%ld,\"msaa_%s_children_hr\":", name, SUCCEEDED(counted) ? count : -1L, name);
        tw_hr(diag, counted);
        IAccessible_Release(acc);
    }
}

void tw_msaa_read(HWND top, tw_nodes *ns, tw_buf *diag)
{
    msaa_walk walk;

    memset(&walk, 0, sizeof walk);
    walk.ns = ns;
    msaa_window(&walk, top, NULL, 0);
    tw_printf(diag, ",\"msaa_objects\":%d,\"msaa_named\":%d,\"msaa_with_default_action\":%d,\"msaa_simple_children\":%d,\"msaa_failures\":%d",
              walk.objects, walk.named, walk.with_action, walk.simple_children, walk.failures);
    tw_puts(diag, ",\"msaa_first_failure\":");
    tw_hr(diag, walk.first_failure);
    probe(top, OBJID_WINDOW, "objid_window", diag);
    probe(top, OBJID_MENU, "objid_menu", diag);
    probe(top, OBJID_TITLEBAR, "objid_titlebar", diag);
    probe(top, OBJID_SYSMENU, "objid_sysmenu", diag);
}

void tw_msaa_act(HWND hwnd, const WCHAR *action, const WCHAR *text, tw_buf *out)
{
    IAccessible *acc = NULL;
    HRESULT hr = AccessibleObjectFromWindow(hwnd, (DWORD)OBJID_CLIENT, &tw_IID_IAccessible, (void **)&acc);
    VARIANT self;
    const char *how = NULL;

    if (FAILED(hr) || !acc) {
        tw_puts(out, "\"done\":false,\"code\":\"failed\",\"why\":\"no accessible object\",\"hr\":");
        tw_hr(out, hr);
        return;
    }
    VariantInit(&self);
    V_VT(&self) = VT_I4;
    V_I4(&self) = CHILDID_SELF;
    if (!lstrcmpiW(action, L"press")) {
        hr = IAccessible_accDoDefaultAction(acc, self);
        how = "IAccessible::accDoDefaultAction";
    } else if (!lstrcmpiW(action, L"type") && text) {
        BSTR value = SysAllocString(text);

        hr = IAccessible_put_accValue(acc, self, value);
        SysFreeString(value);
        how = "IAccessible::put_accValue, which sets the whole value";
    } else if (!lstrcmpiW(action, L"select") || !lstrcmpiW(action, L"focus")) {
        hr = IAccessible_accSelect(acc, !lstrcmpiW(action, L"select") ? 3 : 1, self);
        how = "IAccessible::accSelect";
    } else {
        hr = E_NOTIMPL;
        how = "nothing in IAccessible for this";
    }
    IAccessible_Release(acc);
    tw_printf(out, "\"done\":%s,\"how\":\"%s\",\"hr\":", hr == S_OK ? "true" : "false", how);
    tw_hr(out, hr);
    if (hr != S_OK)
        tw_puts(out, ",\"code\":\"not_offered\"");
}

#else /* TW_STUB */

void tw_msaa_read(HWND top, tw_nodes *ns, tw_buf *diag)
{
    (void)top;
    (void)ns;
    tw_puts(diag, ",\"msaa\":\"not built\"");
}

void tw_msaa_act(HWND hwnd, const WCHAR *action, const WCHAR *text, tw_buf *out)
{
    (void)hwnd;
    (void)action;
    (void)text;
    tw_puts(out, "\"done\":false,\"code\":\"unavailable\",\"why\":\"the MSAA tier was not built\"");
}

#endif
