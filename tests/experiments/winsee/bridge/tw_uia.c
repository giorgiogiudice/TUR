/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * tur-winbridge, the UI Automation tier: what the COM client of
 * uiautomationcore (IUIAutomation) gives for a window. Wine 11.20's source
 * shows which of its methods are written (ElementFromHandle,
 * CreateTreeWalker, the raw view condition, FindAll, the name, the control
 * type, the bounding rectangle, GetCurrentPropertyValueEx) and which are
 * stubs (the ready-made walkers, GetCurrentPattern); this file uses the
 * former and records what the latter answer. Built with -DTW_STUB when the
 * headers at hand do not carry the interfaces; the tier then says so.
 */
#include "tw.h"

#ifndef TW_STUB

#include <uiautomationclient.h>

static const GUID tw_CLSID_CUIAutomation = {0xff48dba4, 0x60ef, 0x4201, {0xaa, 0x87, 0x54, 0x10, 0x3e, 0xef, 0x59, 0x4e}};
static const GUID tw_IID_IUIAutomation = {0x30cbe57d, 0xd9d0, 0x452a, {0xab, 0x13, 0x7a, 0xc5, 0xac, 0x48, 0x25, 0xee}};
static const GUID tw_IID_InvokePattern = {0xfb377fbe, 0x8ea6, 0x46d5, {0x9c, 0x73, 0x64, 0x99, 0x64, 0x2d, 0x30, 0x59}};
static const GUID tw_IID_ValuePattern = {0xa94cd8b1, 0x0844, 0x4cd6, {0x9d, 0x2d, 0x64, 0x05, 0x37, 0xab, 0x39, 0xe9}};

#define TW_UIA_IS_ENABLED 30010
#define TW_UIA_HAS_FOCUS 30008
#define TW_UIA_CLASS_NAME 30012
#define TW_UIA_IS_PASSWORD 30019
#define TW_UIA_NATIVE_WINDOW 30020
#define TW_UIA_IS_OFFSCREEN 30022
#define TW_UIA_VALUE 30045
#define TW_UIA_INVOKE_PATTERN 10000
#define TW_UIA_VALUE_PATTERN 10002
#define TW_UIA_LEGACY_PATTERN 10018

static int role_from_control_type(long type)
{
    switch (type) {
    case 50000: return TW_ROLE_PUSH_BUTTON;
    case 50002: return TW_ROLE_CHECK_BOX;
    case 50003: return TW_ROLE_COMBO_BOX;
    case 50004: return TW_ROLE_TEXT;
    case 50005: return TW_ROLE_LINK;
    case 50006: return TW_ROLE_IMAGE;
    case 50007: return TW_ROLE_LIST_ITEM;
    case 50008: return TW_ROLE_LIST;
    case 50009: return TW_ROLE_MENU;
    case 50010: return TW_ROLE_MENU_BAR;
    case 50011: return TW_ROLE_MENU_ITEM;
    case 50012: return TW_ROLE_PROGRESS_BAR;
    case 50013: return TW_ROLE_RADIO_BUTTON;
    case 50014: return TW_ROLE_SCROLL_BAR;
    case 50015: return TW_ROLE_SLIDER;
    case 50016: return TW_ROLE_SPIN_BUTTON;
    case 50017: return TW_ROLE_STATUS_BAR;
    case 50018: return TW_ROLE_PAGE_TAB_LIST;
    case 50019: return TW_ROLE_PAGE_TAB;
    case 50020: return TW_ROLE_LABEL;
    case 50021: return TW_ROLE_TOOL_BAR;
    case 50022: return TW_ROLE_TOOL_TIP;
    case 50023: return TW_ROLE_TREE;
    case 50024: return TW_ROLE_TREE_ITEM;
    case 50026: return TW_ROLE_PANEL;
    case 50030: return TW_ROLE_TEXT;
    case 50031: return TW_ROLE_PUSH_BUTTON;
    case 50032: return TW_ROLE_WINDOW;
    case 50033: return TW_ROLE_PANEL;
    case 50035: return TW_ROLE_TABLE_COLUMN_HEADER;
    case 50037: return TW_ROLE_TITLE_BAR;
    case 50038: return TW_ROLE_SEPARATOR;
    default: return TW_ROLE_UNKNOWN;
    }
}

typedef struct {
    tw_nodes *ns;
    IUIAutomationTreeWalker *walker;
    int next_id;
    int named;
    int property_failures;
    HRESULT first_property_failure;
} uia_walk;

static HRESULT property(uia_walk *w, IUIAutomationElement *element, int id, VARIANT *value)
{
    HRESULT hr;

    VariantInit(value);
    hr = IUIAutomationElement_GetCurrentPropertyValueEx(element, id, TRUE, value);
    if (FAILED(hr)) {
        w->property_failures++;
        if (!w->first_property_failure)
            w->first_property_failure = hr;
    }
    return hr;
}

static void uia_fill(uia_walk *w, tw_node *node, IUIAutomationElement *element)
{
    BSTR name = NULL;
    CONTROLTYPEID type = 0;
    RECT rc;
    VARIANT value;

    if (SUCCEEDED(IUIAutomationElement_get_CurrentName(element, &name)) && name && name[0]) {
        tw_copy(node->name, TW_NAME, name);
        node->name_from = "uia";
        w->named++;
    }
    SysFreeString(name);
    if (SUCCEEDED(IUIAutomationElement_get_CurrentControlType(element, &type))) {
        node->native_role = (int)type;
        node->role = role_from_control_type(type);
    }
    SetRectEmpty(&rc);
    if (SUCCEEDED(IUIAutomationElement_get_CurrentBoundingRectangle(element, &rc)) && rc.right > rc.left &&
        rc.bottom > rc.top) {
        node->rc = rc;
        node->has_rc = 1;
    }
    if (SUCCEEDED(property(w, element, TW_UIA_CLASS_NAME, &value)) && V_VT(&value) == VT_BSTR && V_BSTR(&value))
        tw_copy(node->cls, 64, V_BSTR(&value));
    VariantClear(&value);
    if (SUCCEEDED(property(w, element, TW_UIA_IS_ENABLED, &value)) && V_VT(&value) == VT_BOOL && !V_BOOL(&value))
        node->states |= TW_ST_DISABLED;
    VariantClear(&value);
    if (SUCCEEDED(property(w, element, TW_UIA_HAS_FOCUS, &value)) && V_VT(&value) == VT_BOOL && V_BOOL(&value))
        node->states |= TW_ST_FOCUSED;
    VariantClear(&value);
    if (SUCCEEDED(property(w, element, TW_UIA_IS_OFFSCREEN, &value)) && V_VT(&value) == VT_BOOL && V_BOOL(&value))
        node->states |= TW_ST_OFFSCREEN;
    VariantClear(&value);
    if (SUCCEEDED(property(w, element, TW_UIA_NATIVE_WINDOW, &value)) && V_VT(&value) == VT_I4)
        node->hwnd = (HWND)(ULONG_PTR)(ULONG)V_I4(&value);
    VariantClear(&value);
    if (SUCCEEDED(property(w, element, TW_UIA_IS_PASSWORD, &value)) && V_VT(&value) == VT_BOOL && V_BOOL(&value)) {
        node->role = TW_ROLE_PASSWORD_TEXT;
        node->text_withheld = 1;
    }
    VariantClear(&value);
    if (!node->text_withheld && SUCCEEDED(property(w, element, TW_UIA_VALUE, &value)) && V_VT(&value) == VT_BSTR &&
        V_BSTR(&value) && V_BSTR(&value)[0]) {
        tw_copy(node->text, TW_TEXT, V_BSTR(&value));
        node->has_text = 1;
    }
    VariantClear(&value);
}

static tw_node *uia_node(uia_walk *w, IUIAutomationElement *element, const WCHAR *parent_id, int depth, WCHAR *id)
{
    tw_node *node = tw_add(w->ns);

    if (!node)
        return NULL;
    tw_copy(node->id, TW_ID, L"u");
    tw_cat_int(node->id, TW_ID, w->next_id++);
    tw_copy(id, TW_ID, node->id);
    if (parent_id)
        tw_copy(node->parent, TW_ID, parent_id);
    node->depth = depth;
    node->kind = 'w';
    uia_fill(w, node, element);
    return node;
}

static void uia_children(uia_walk *w, IUIAutomationElement *element, const WCHAR *parent_id, int depth)
{
    IUIAutomationElement *child = NULL;

    if (depth > 25 || w->next_id > 3500)
        return;
    if (FAILED(IUIAutomationTreeWalker_GetFirstChildElement(w->walker, element, &child)))
        return;
    while (child) {
        IUIAutomationElement *next = NULL;
        WCHAR id[TW_ID];

        if (!uia_node(w, child, parent_id, depth, id)) {
            IUIAutomationElement_Release(child);
            return;
        }
        uia_children(w, child, id, depth + 1);
        if (FAILED(IUIAutomationTreeWalker_GetNextSiblingElement(w->walker, child, &next)))
            next = NULL;
        IUIAutomationElement_Release(child);
        child = next;
        if (w->next_id > 3500 && child) {
            IUIAutomationElement_Release(child);
            child = NULL;
        }
    }
}

static void diag_hr(tw_buf *diag, const char *name, HRESULT hr)
{
    tw_printf(diag, ",\"%s\":", name);
    tw_hr(diag, hr);
}

void tw_uia_read(HWND top, tw_nodes *ns, tw_buf *diag)
{
    IUIAutomation *uia = NULL;
    IUIAutomationElement *root = NULL;
    IUIAutomationCondition *raw = NULL, *all = NULL;
    IUIAutomationTreeWalker *ready = NULL;
    IUIAutomationElementArray *found = NULL;
    IUnknown *pattern = NULL;
    uia_walk walk;
    HRESULT hr;
    WCHAR id[TW_ID];
    int length = -1;

    memset(&walk, 0, sizeof walk);
    walk.ns = ns;
    hr = CoCreateInstance(&tw_CLSID_CUIAutomation, NULL, CLSCTX_INPROC_SERVER, &tw_IID_IUIAutomation, (void **)&uia);
    diag_hr(diag, "uia_create", hr);
    if (FAILED(hr) || !uia)
        return;
    hr = IUIAutomation_ElementFromHandle(uia, (UIA_HWND)top, &root);
    diag_hr(diag, "uia_element_from_handle", hr);
    if (FAILED(hr) || !root) {
        IUIAutomation_Release(uia);
        return;
    }
    /* The walkers a client would take ready-made. */
    hr = IUIAutomation_get_RawViewWalker(uia, &ready);
    diag_hr(diag, "uia_raw_view_walker", hr);
    if (SUCCEEDED(hr) && ready)
        IUIAutomationTreeWalker_Release(ready);
    hr = IUIAutomation_get_RawViewCondition(uia, &raw);
    diag_hr(diag, "uia_raw_view_condition", hr);
    if (SUCCEEDED(hr) && raw) {
        hr = IUIAutomation_CreateTreeWalker(uia, raw, &walk.walker);
        diag_hr(diag, "uia_create_tree_walker", hr);
    }
    if (uia_node(&walk, root, NULL, 0, id) && walk.walker)
        uia_children(&walk, root, id, 1);

    /* The same tree by FindAll, as a count to compare with. */
    hr = IUIAutomation_CreateTrueCondition(uia, &all);
    if (SUCCEEDED(hr) && all) {
        hr = IUIAutomationElement_FindAll(root, TreeScope_Descendants, all, &found);
        diag_hr(diag, "uia_find_all", hr);
        if (SUCCEEDED(hr) && found) {
            IUIAutomationElementArray_get_Length(found, &length);
            IUIAutomationElementArray_Release(found);
        }
        IUIAutomationCondition_Release(all);
    }
    /* Whether anything can be done through it. */
    hr = IUIAutomationElement_GetCurrentPattern(root, TW_UIA_LEGACY_PATTERN, &pattern);
    diag_hr(diag, "uia_get_current_pattern", hr);
    if (SUCCEEDED(hr) && pattern)
        IUnknown_Release(pattern);

    tw_printf(diag, ",\"uia_elements\":%d,\"uia_named\":%d,\"uia_find_all_descendants\":%d,\"uia_property_failures\":%d",
              walk.next_id, walk.named, length, walk.property_failures);
    diag_hr(diag, "uia_first_property_failure", walk.first_property_failure);
    if (walk.walker)
        IUIAutomationTreeWalker_Release(walk.walker);
    if (raw)
        IUIAutomationCondition_Release(raw);
    IUIAutomationElement_Release(root);
    IUIAutomation_Release(uia);
}

void tw_uia_act(HWND hwnd, const WCHAR *action, tw_buf *out)
{
    IUIAutomation *uia = NULL;
    IUIAutomationElement *element = NULL;
    IUnknown *pattern = NULL;
    HRESULT hr, invoke_hr, value_hr, legacy_hr, done_hr = E_NOTIMPL;
    int done = 0;

    hr = CoCreateInstance(&tw_CLSID_CUIAutomation, NULL, CLSCTX_INPROC_SERVER, &tw_IID_IUIAutomation, (void **)&uia);
    if (SUCCEEDED(hr) && uia)
        hr = IUIAutomation_ElementFromHandle(uia, (UIA_HWND)hwnd, &element);
    if (FAILED(hr) || !element) {
        tw_puts(out, "\"done\":false,\"code\":\"failed\",\"why\":\"no automation element\",\"hr\":");
        tw_hr(out, hr);
        if (uia)
            IUIAutomation_Release(uia);
        return;
    }
    invoke_hr = IUIAutomationElement_GetCurrentPattern(element, TW_UIA_INVOKE_PATTERN, &pattern);
    if (SUCCEEDED(invoke_hr) && pattern) {
        IUIAutomationInvokePattern *invoke = NULL;

        if (!lstrcmpiW(action, L"press") &&
            SUCCEEDED(IUnknown_QueryInterface(pattern, &tw_IID_InvokePattern, (void **)&invoke)) && invoke) {
            done_hr = IUIAutomationInvokePattern_Invoke(invoke);
            done = done_hr == S_OK;
            IUIAutomationInvokePattern_Release(invoke);
        }
        IUnknown_Release(pattern);
    }
    pattern = NULL;
    value_hr = IUIAutomationElement_GetCurrentPattern(element, TW_UIA_VALUE_PATTERN, &pattern);
    if (SUCCEEDED(value_hr) && pattern) {
        IUnknown *probe = NULL;

        if (SUCCEEDED(IUnknown_QueryInterface(pattern, &tw_IID_ValuePattern, (void **)&probe)) && probe)
            IUnknown_Release(probe);
        IUnknown_Release(pattern);
    }
    pattern = NULL;
    legacy_hr = IUIAutomationElement_GetCurrentPattern(element, TW_UIA_LEGACY_PATTERN, &pattern);
    if (SUCCEEDED(legacy_hr) && pattern)
        IUnknown_Release(pattern);
    tw_printf(out, "\"done\":%s,\"how\":\"IUIAutomationElement::GetCurrentPattern\"", done ? "true" : "false");
    if (!done)
        tw_puts(out, ",\"code\":\"not_offered\"");
    tw_puts(out, ",\"invoke_pattern_hr\":");
    tw_hr(out, invoke_hr);
    tw_puts(out, ",\"value_pattern_hr\":");
    tw_hr(out, value_hr);
    tw_puts(out, ",\"legacy_pattern_hr\":");
    tw_hr(out, legacy_hr);
    tw_puts(out, ",\"hr\":");
    tw_hr(out, done_hr);
    IUIAutomationElement_Release(element);
    IUIAutomation_Release(uia);
}

#else /* TW_STUB */

void tw_uia_read(HWND top, tw_nodes *ns, tw_buf *diag)
{
    (void)top;
    (void)ns;
    tw_puts(diag, ",\"uia\":\"not built\"");
}

void tw_uia_act(HWND hwnd, const WCHAR *action, tw_buf *out)
{
    (void)hwnd;
    (void)action;
    tw_puts(out, "\"done\":false,\"code\":\"unavailable\",\"why\":\"the UI Automation tier was not built\"");
}

#endif
