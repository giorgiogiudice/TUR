/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * tur-winbridge, COM automation: the scriptable objects registered in the
 * prefix, the members of an object's automation interface as its type
 * information declares them (from which the experiment's harness makes
 * typed tool definitions), and a call through IDispatch.
 *
 * The bridge calls only what its parent asks for; the parent decides by
 * policy. Built with -DTW_STUB when this does not compile.
 */
#include "tw.h"

#ifndef TW_STUB

#include <oaidl.h>
#include <oleauto.h>

static const GUID tw_IID_IDispatch = {0x00020400, 0x0000, 0x0000, {0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46}};
static const GUID tw_IID_NULL = {0x00000000, 0x0000, 0x0000, {0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00}};

#define TW_LOCALE 0x0409

/* The default value of root\key\sub. Returns whether the key is there. */
static int sub_value(HKEY root, const WCHAR *key, const WCHAR *sub, WCHAR *out, DWORD cap)
{
    WCHAR path[400];
    HKEY handle;
    DWORD type = 0, size = (cap - 1) * sizeof(WCHAR);
    LONG status;

    out[0] = 0;
    tw_copy(path, 400, key);
    if (sub && sub[0]) {
        tw_cat(path, 400, L"\\");
        tw_cat(path, 400, sub);
    }
    if (RegOpenKeyExW(root, path, 0, KEY_READ, &handle) != ERROR_SUCCESS)
        return 0;
    status = RegQueryValueExW(handle, NULL, NULL, &type, (BYTE *)out, &size);
    RegCloseKey(handle);
    if (status != ERROR_SUCCESS || (type != REG_SZ && type != REG_EXPAND_SZ))
        out[0] = 0;
    else
        out[(size / sizeof(WCHAR)) < cap ? (size / sizeof(WCHAR)) : cap - 1] = 0;
    return 1;
}

void tw_com_list(int max, tw_buf *out)
{
    HKEY classes, libraries;
    DWORD index;
    int total = 0, with_progid = 0, with_typelib = 0, programmable = 0, local = 0, listed = 0, libs = 0;

    tw_puts(out, "\"classes\":[");
    if (RegOpenKeyExW(HKEY_CLASSES_ROOT, L"CLSID", 0, KEY_READ, &classes) == ERROR_SUCCESS) {
        for (index = 0; (int)index < max; index++) {
            WCHAR name[160], progid[200], typelib[100], server[400], unused[8];
            DWORD size = 160;
            int has_progid, has_typelib, is_programmable, is_local;

            if (RegEnumKeyExW(classes, index, name, &size, NULL, NULL, NULL, NULL) != ERROR_SUCCESS)
                break;
            total++;
            has_progid = sub_value(classes, name, L"ProgID", progid, 200) && progid[0];
            has_typelib = sub_value(classes, name, L"TypeLib", typelib, 100) && typelib[0];
            is_programmable = sub_value(classes, name, L"Programmable", unused, 8);
            is_local = sub_value(classes, name, L"LocalServer32", server, 400) && server[0];
            if (!is_local)
                sub_value(classes, name, L"InprocServer32", server, 400);
            with_progid += has_progid;
            with_typelib += has_typelib;
            programmable += is_programmable;
            local += is_local;
            if (has_progid && (has_typelib || is_programmable) && listed < 400) {
                if (listed++)
                    tw_putc(out, ',');
                tw_puts(out, "{\"clsid\":");
                tw_jstr(out, name);
                tw_puts(out, ",\"progid\":");
                tw_jstr(out, progid);
                tw_puts(out, ",\"typelib\":");
                tw_jstr(out, typelib);
                tw_puts(out, ",\"server\":");
                tw_jstr(out, server);
                tw_printf(out, ",\"local_server\":%s,\"programmable\":%s}", is_local ? "true" : "false",
                          is_programmable ? "true" : "false");
            }
        }
        RegCloseKey(classes);
    }
    tw_puts(out, "],\"type_libraries\":[");
    if (RegOpenKeyExW(HKEY_CLASSES_ROOT, L"TypeLib", 0, KEY_READ, &libraries) == ERROR_SUCCESS) {
        for (index = 0; (int)index < max; index++) {
            WCHAR name[160], version[64], title[300], path[300];
            DWORD size = 160, version_size = 64;
            HKEY library;

            if (RegEnumKeyExW(libraries, index, name, &size, NULL, NULL, NULL, NULL) != ERROR_SUCCESS)
                break;
            libs++;
            if (libs > 400)
                continue;
            version[0] = 0;
            title[0] = 0;
            if (RegOpenKeyExW(libraries, name, 0, KEY_READ, &library) == ERROR_SUCCESS) {
                if (RegEnumKeyExW(library, 0, version, &version_size, NULL, NULL, NULL, NULL) != ERROR_SUCCESS)
                    version[0] = 0;
                RegCloseKey(library);
            }
            if (version[0]) {
                tw_copy(path, 300, name);
                tw_cat(path, 300, L"\\");
                tw_cat(path, 300, version);
                sub_value(libraries, path, NULL, title, 300);
            }
            if (libs > 1)
                tw_putc(out, ',');
            tw_puts(out, "{\"libid\":");
            tw_jstr(out, name);
            tw_puts(out, ",\"version\":");
            tw_jstr(out, version);
            tw_puts(out, ",\"name\":");
            tw_jstr(out, title);
            tw_putc(out, '}');
        }
        RegCloseKey(libraries);
    }
    tw_printf(out, "],\"clsids\":%d,\"with_progid\":%d,\"with_typelib\":%d,\"programmable\":%d,\"local_servers\":%d,\"type_library_count\":%d",
              total, with_progid, with_typelib, programmable, local, libs);
}

static void type_name(ITypeInfo *info, const TYPEDESC *type, WCHAR *out, size_t cap, int depth)
{
    switch (type->vt) {
    case VT_EMPTY: tw_cat(out, cap, L"empty"); break;
    case VT_NULL: tw_cat(out, cap, L"null"); break;
    case VT_I1: tw_cat(out, cap, L"int8"); break;
    case VT_UI1: tw_cat(out, cap, L"uint8"); break;
    case VT_I2: tw_cat(out, cap, L"int16"); break;
    case VT_UI2: tw_cat(out, cap, L"uint16"); break;
    case VT_I4: case VT_INT: tw_cat(out, cap, L"int32"); break;
    case VT_UI4: case VT_UINT: tw_cat(out, cap, L"uint32"); break;
    case VT_I8: tw_cat(out, cap, L"int64"); break;
    case VT_UI8: tw_cat(out, cap, L"uint64"); break;
    case VT_R4: tw_cat(out, cap, L"float"); break;
    case VT_R8: tw_cat(out, cap, L"double"); break;
    case VT_CY: tw_cat(out, cap, L"currency"); break;
    case VT_DATE: tw_cat(out, cap, L"date"); break;
    case VT_BSTR: case VT_LPSTR: case VT_LPWSTR: tw_cat(out, cap, L"string"); break;
    case VT_DISPATCH: tw_cat(out, cap, L"object"); break;
    case VT_UNKNOWN: tw_cat(out, cap, L"unknown"); break;
    case VT_ERROR: tw_cat(out, cap, L"error"); break;
    case VT_BOOL: tw_cat(out, cap, L"boolean"); break;
    case VT_VARIANT: tw_cat(out, cap, L"variant"); break;
    case VT_VOID: tw_cat(out, cap, L"void"); break;
    case VT_HRESULT: tw_cat(out, cap, L"hresult"); break;
    case VT_PTR:
        if (depth < 4 && type->lptdesc)
            type_name(info, type->lptdesc, out, cap, depth + 1);
        else
            tw_cat(out, cap, L"pointer");
        tw_cat(out, cap, L"*");
        break;
    case VT_SAFEARRAY:
        tw_cat(out, cap, L"array of ");
        if (depth < 4 && type->lptdesc)
            type_name(info, type->lptdesc, out, cap, depth + 1);
        break;
    case VT_USERDEFINED: {
        ITypeInfo *other = NULL;
        BSTR name = NULL;

        if (SUCCEEDED(ITypeInfo_GetRefTypeInfo(info, type->hreftype, &other)) && other) {
            if (SUCCEEDED(ITypeInfo_GetDocumentation(other, MEMBERID_NIL, &name, NULL, NULL, NULL)) && name)
                tw_cat(out, cap, name);
            else
                tw_cat(out, cap, L"user type");
            SysFreeString(name);
            ITypeInfo_Release(other);
        } else {
            tw_cat(out, cap, L"user type");
        }
        break;
    }
    default:
        tw_cat(out, cap, L"vt");
        tw_cat_int(out, cap, (long)type->vt);
        break;
    }
}

static struct {
    WCHAR progid[120];
    IDispatch *object;
} objects[8];

static IDispatch *object_for(const WCHAR *progid, HRESULT *hr)
{
    CLSID clsid;
    int i, free_slot = -1;
    IDispatch *object = NULL;

    for (i = 0; i < 8; i++) {
        if (objects[i].object && !lstrcmpiW(objects[i].progid, progid)) {
            *hr = S_OK;
            return objects[i].object;
        }
        if (!objects[i].object && free_slot < 0)
            free_slot = i;
    }
    *hr = CLSIDFromProgID(progid, &clsid);
    if (FAILED(*hr))
        return NULL;
    *hr = CoCreateInstance(&clsid, NULL, CLSCTX_INPROC_SERVER | CLSCTX_LOCAL_SERVER, &tw_IID_IDispatch, (void **)&object);
    if (FAILED(*hr) || !object)
        return NULL;
    if (free_slot >= 0) {
        tw_copy(objects[free_slot].progid, 120, progid);
        objects[free_slot].object = object;
    }
    return object;
}

void tw_com_typeinfo(const WCHAR *progid, tw_buf *out)
{
    IDispatch *object;
    ITypeInfo *info = NULL;
    ITypeLib *library = NULL;
    TYPEATTR *attr = NULL;
    HRESULT hr;
    BSTR name = NULL, doc = NULL;
    UINT index = 0, i, count = 0;
    int first = 1;

    if (!progid) {
        tw_puts(out, "\"error\":\"no progid\"");
        return;
    }
    tw_puts(out, "\"progid\":");
    tw_jstr(out, progid);
    object = object_for(progid, &hr);
    tw_puts(out, ",\"create_hr\":");
    tw_hr(out, hr);
    if (!object)
        return;
    hr = IDispatch_GetTypeInfoCount(object, &count);
    tw_printf(out, ",\"type_info_count\":%u", count);
    hr = IDispatch_GetTypeInfo(object, 0, TW_LOCALE, &info);
    tw_puts(out, ",\"type_info_hr\":");
    tw_hr(out, hr);
    if (FAILED(hr) || !info)
        return;
    if (SUCCEEDED(ITypeInfo_GetDocumentation(info, MEMBERID_NIL, &name, &doc, NULL, NULL))) {
        tw_puts(out, ",\"interface\":");
        tw_jstr(out, name);
        tw_puts(out, ",\"interface_doc\":");
        tw_jstr(out, doc);
    }
    SysFreeString(name);
    SysFreeString(doc);
    name = doc = NULL;
    if (SUCCEEDED(ITypeInfo_GetContainingTypeLib(info, &library, &index)) && library) {
        if (SUCCEEDED(ITypeLib_GetDocumentation(library, -1, &name, &doc, NULL, NULL))) {
            tw_puts(out, ",\"library\":");
            tw_jstr(out, name);
            tw_puts(out, ",\"library_doc\":");
            tw_jstr(out, doc);
        }
        SysFreeString(name);
        SysFreeString(doc);
        name = doc = NULL;
        tw_printf(out, ",\"library_types\":%u", ITypeLib_GetTypeInfoCount(library));
        ITypeLib_Release(library);
    }
    hr = ITypeInfo_GetTypeAttr(info, &attr);
    if (FAILED(hr) || !attr) {
        ITypeInfo_Release(info);
        return;
    }
    tw_printf(out, ",\"type_kind\":%d,\"functions\":%u,\"members\":[", (int)attr->typekind, (unsigned)attr->cFuncs);
    for (i = 0; i < attr->cFuncs && i < 400; i++) {
        FUNCDESC *function = NULL;
        BSTR names[40];
        UINT got = 0, p;
        WCHAR type[200];
        const char *kind;

        if (FAILED(ITypeInfo_GetFuncDesc(info, i, &function)) || !function)
            continue;
        memset(names, 0, sizeof names);
        ITypeInfo_GetNames(info, function->memid, names, 40, &got);
        doc = NULL;
        ITypeInfo_GetDocumentation(info, function->memid, NULL, &doc, NULL, NULL);
        switch (function->invkind) {
        case INVOKE_PROPERTYGET: kind = "get"; break;
        case INVOKE_PROPERTYPUT: kind = "put"; break;
        case INVOKE_PROPERTYPUTREF: kind = "putref"; break;
        default: kind = "method"; break;
        }
        if (!first)
            tw_putc(out, ',');
        first = 0;
        tw_puts(out, "{\"name\":");
        tw_jstr(out, got ? names[0] : L"");
        type[0] = 0;
        type_name(info, &function->elemdescFunc.tdesc, type, 200, 0);
        tw_printf(out, ",\"kind\":\"%s\",\"memid\":%ld,\"restricted\":%s,\"hidden\":%s,\"returns\":", kind,
                  (long)function->memid, (function->wFuncFlags & FUNCFLAG_FRESTRICTED) ? "true" : "false",
                  (function->wFuncFlags & FUNCFLAG_FHIDDEN) ? "true" : "false");
        tw_jstr(out, type);
        tw_puts(out, ",\"doc\":");
        tw_jstr(out, doc);
        tw_printf(out, ",\"optional_params\":%d,\"params\":[", (int)function->cParamsOpt);
        for (p = 0; p < (UINT)function->cParams && p < 30; p++) {
            const ELEMDESC *element = &function->lprgelemdescParam[p];
            USHORT flags = element->paramdesc.wParamFlags;

            if (p)
                tw_putc(out, ',');
            tw_puts(out, "{\"name\":");
            tw_jstr(out, p + 1 < got ? names[p + 1] : L"");
            type[0] = 0;
            type_name(info, &element->tdesc, type, 200, 0);
            tw_puts(out, ",\"type\":");
            tw_jstr(out, type);
            tw_printf(out, ",\"in\":%s,\"out\":%s,\"retval\":%s,\"optional\":%s}",
                      (flags & PARAMFLAG_FIN) || !(flags & PARAMFLAG_FOUT) ? "true" : "false",
                      (flags & PARAMFLAG_FOUT) ? "true" : "false", (flags & PARAMFLAG_FRETVAL) ? "true" : "false",
                      (flags & PARAMFLAG_FOPT) ? "true" : "false");
        }
        tw_puts(out, "]}");
        for (p = 0; p < got; p++)
            SysFreeString(names[p]);
        SysFreeString(doc);
        ITypeInfo_ReleaseFuncDesc(info, function);
    }
    tw_putc(out, ']');
    ITypeInfo_ReleaseTypeAttr(info, attr);
    ITypeInfo_Release(info);
}

static void put_variant(tw_buf *out, const VARIANT *value)
{
    switch (V_VT(value)) {
    case VT_EMPTY:
    case VT_NULL:
        tw_puts(out, "null");
        break;
    case VT_BSTR:
        tw_jstr(out, V_BSTR(value));
        break;
    case VT_I2:
        tw_printf(out, "%d", (int)V_I2(value));
        break;
    case VT_I4:
    case VT_INT:
        tw_printf(out, "%ld", (long)V_I4(value));
        break;
    case VT_UI4:
    case VT_UINT:
        tw_printf(out, "%lu", (unsigned long)V_UI4(value));
        break;
    case VT_BOOL:
        tw_puts(out, V_BOOL(value) ? "true" : "false");
        break;
    case VT_R8:
    case VT_DATE:
        tw_printf(out, "%g", V_R8(value));
        break;
    case VT_DISPATCH:
        tw_puts(out, "\"<object>\"");
        break;
    default:
        tw_printf(out, "\"<variant of type %d>\"", (int)V_VT(value));
        break;
    }
}

void tw_com_invoke(const WCHAR *progid, const WCHAR *member, const WCHAR *kind, const WCHAR **args, int nargs,
                   tw_buf *out)
{
    IDispatch *object;
    HRESULT hr;
    DISPID dispid = 0;
    LPOLESTR names[1];
    VARIANT argv[4], result;
    DISPPARAMS params;
    EXCEPINFO exception;
    UINT bad = 0;
    int i;

    if (!progid || !member) {
        tw_puts(out, "\"error\":\"no progid or member\"");
        return;
    }
    object = object_for(progid, &hr);
    tw_puts(out, "\"create_hr\":");
    tw_hr(out, hr);
    if (!object)
        return;
    names[0] = (LPOLESTR)member;
    hr = IDispatch_GetIDsOfNames(object, &tw_IID_NULL, names, 1, TW_LOCALE, &dispid);
    tw_puts(out, ",\"name_hr\":");
    tw_hr(out, hr);
    if (FAILED(hr))
        return;
    if (nargs > 4)
        nargs = 4;
    /* The arguments of IDispatch::Invoke go last first. "i:" marks a
     * number, "b:" a boolean; anything else is text. */
    for (i = 0; i < nargs; i++) {
        VARIANT *slot = &argv[nargs - 1 - i];
        const WCHAR *text = args[i] ? args[i] : L"";

        VariantInit(slot);
        if (text[0] == 'i' && text[1] == ':') {
            V_VT(slot) = VT_I4;
            V_I4(slot) = wcstol(text + 2, NULL, 10);
        } else if (text[0] == 'b' && text[1] == ':') {
            V_VT(slot) = VT_BOOL;
            V_BOOL(slot) = !lstrcmpiW(text + 2, L"true") ? VARIANT_TRUE : VARIANT_FALSE;
        } else {
            V_VT(slot) = VT_BSTR;
            V_BSTR(slot) = SysAllocString(text[0] == 's' && text[1] == ':' ? text + 2 : text);
        }
    }
    memset(&params, 0, sizeof params);
    params.rgvarg = nargs ? argv : NULL;
    params.cArgs = (UINT)nargs;
    memset(&exception, 0, sizeof exception);
    VariantInit(&result);
    hr = IDispatch_Invoke(object, dispid, &tw_IID_NULL, TW_LOCALE,
                          (kind && !lstrcmpiW(kind, L"get")) ? DISPATCH_PROPERTYGET : DISPATCH_METHOD, &params,
                          &result, &exception, &bad);
    tw_printf(out, ",\"dispid\":%ld,\"hr\":", (long)dispid);
    tw_hr(out, hr);
    tw_puts(out, ",\"result\":");
    put_variant(out, &result);
    if (hr == DISP_E_EXCEPTION) {
        tw_puts(out, ",\"exception\":");
        tw_jstr(out, exception.bstrDescription);
    }
    SysFreeString(exception.bstrSource);
    SysFreeString(exception.bstrDescription);
    SysFreeString(exception.bstrHelpFile);
    VariantClear(&result);
    for (i = 0; i < nargs; i++)
        VariantClear(&argv[i]);
}

#else /* TW_STUB */

void tw_com_list(int max, tw_buf *out)
{
    (void)max;
    tw_puts(out, "\"error\":\"COM automation was not built\"");
}

void tw_com_typeinfo(const WCHAR *progid, tw_buf *out)
{
    (void)progid;
    tw_puts(out, "\"error\":\"COM automation was not built\"");
}

void tw_com_invoke(const WCHAR *progid, const WCHAR *member, const WCHAR *kind, const WCHAR **args, int nargs,
                   tw_buf *out)
{
    (void)progid;
    (void)member;
    (void)kind;
    (void)args;
    (void)nargs;
    tw_puts(out, "\"error\":\"COM automation was not built\"");
}

#endif
