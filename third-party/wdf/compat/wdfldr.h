/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md): a stand-in, written for this
   build, for the WDK's wdfldr.h, which is not in Microsoft's MIT-licensed
   sources. The loader's own structures (WDF_BIND_INFO, WDF_LIBRARY_INFO,
   CLIENT_INFO, WDF_LOADER_INTERFACE) are in the sources' fxldr.h; this file
   holds the types fxldr.h expects from here, written from their use in the
   sources. Class-extension binding is declared for completeness; the TUR
   loader does not support class extensions. */
#ifndef _TUR_WDFLDR_H_
#define _TUR_WDFLDR_H_

#ifdef __cplusplus
extern "C" {
#endif

typedef ULONG WDF_MAJOR_VERSION;
typedef ULONG WDF_MINOR_VERSION;
typedef ULONG WDF_BUILD_NUMBER;

/* The framework hands its client a PWDF_DRIVER_GLOBALS through this type. */
typedef struct _WDF_COMPONENT_GLOBALS *PWDF_COMPONENT_GLOBALS;

typedef size_t WDF_STRUCT_INFO, *PWDF_STRUCT_INFO;

typedef struct _WDF_INTERFACE_HEADER {
    const GUID *InterfaceType;
    ULONG InterfaceSize;
} WDF_INTERFACE_HEADER, *PWDF_INTERFACE_HEADER;

typedef
NTSTATUS
(*PWDF_LDR_DIAGNOSTICS_VALUE_BY_NAME_AS_ULONG)(
    _In_ PUNICODE_STRING ValueName,
    _Out_ PULONG Value
    );

/* Reads HKLM\System\CurrentControlSet\Control\Wdf\Kmdf\Diagnostics\<name>;
   implemented in tur-wdf-missing.cpp. */
NTSTATUS
WdfLdrDiagnosticsValueByNameAsULONG(
    _In_ PUNICODE_STRING ValueName,
    _Out_ PULONG Value
    );

typedef struct _WDF_CLASS_VERSION {
    WDF_MAJOR_VERSION Major;
    WDF_MINOR_VERSION Minor;
    WDF_BUILD_NUMBER Build;
} WDF_CLASS_VERSION;

typedef VOID (*PFN_WDF_CLASS_EXPORT)(VOID);
typedef struct _WDF_CLASS_BIND_INFO *PWDF_CLASS_BIND_INFO;
typedef struct _WDF_BIND_INFO *PWDF_BIND_INFO;

typedef
NTSTATUS
(*PFN_WDF_CLIENT_BIND_CLASS)(
    PWDF_BIND_INFO BindInfo,
    PWDF_COMPONENT_GLOBALS Globals,
    PWDF_CLASS_BIND_INFO ClassInfo
    );

typedef
VOID
(*PFN_WDF_CLIENT_UNBIND_CLASS)(
    PWDF_BIND_INFO BindInfo,
    PWDF_COMPONENT_GLOBALS Globals,
    PWDF_CLASS_BIND_INFO ClassInfo
    );

typedef struct _WDF_CLASS_BIND_INFO {
    ULONG Size;
    PWCHAR ClassName;
    WDF_CLASS_VERSION Version;
    PFN_WDF_CLASS_EXPORT *FunctionTable;
    ULONG FunctionTableCount;
    PVOID ClassBindInfo;
    PFN_WDF_CLIENT_BIND_CLASS ClientBindClass;
    PFN_WDF_CLIENT_UNBIND_CLASS ClientUnbindClass;
    PVOID ClassModule;
} WDF_CLASS_BIND_INFO;

typedef struct _WDF_CLASS_BIND_INFO2 {
    WDF_CLASS_BIND_INFO V1;
    WDF_MINOR_VERSION *MinimumVersionRequired;
    BOOLEAN *ClientVersionHigherThanClass;
    ULONG *FunctionTableCountPtr;
} WDF_CLASS_BIND_INFO2, *PWDF_CLASS_BIND_INFO2;

typedef struct _WDF_CLASS_EXTENSION_DESCRIPTOR {
    ULONG Size;
    PCUNICODE_STRING ClassName;
    WDF_CLASS_VERSION Version;
} WDF_CLASS_EXTENSION_DESCRIPTOR, *PWDF_CLASS_EXTENSION_DESCRIPTOR;

typedef NTSTATUS (*PFN_CLASS_LIBRARY_INIT)(VOID);
typedef VOID (*PFN_CLASS_LIBRARY_DEINIT)(VOID);
typedef NTSTATUS (*PFN_CLASS_LIBRARY_BIND_CLIENT)(PWDF_BIND_INFO, PWDF_COMPONENT_GLOBALS, PWDF_CLASS_BIND_INFO);
typedef VOID (*PFN_CLASS_LIBRARY_UNBIND_CLIENT)(PWDF_BIND_INFO, PWDF_COMPONENT_GLOBALS, PWDF_CLASS_BIND_INFO);

typedef struct _WDF_CLASS_LIBRARY_INFO {
    ULONG Size;
    WDF_CLASS_VERSION Version;
    PFN_CLASS_LIBRARY_INIT ClassLibraryInitialize;
    PFN_CLASS_LIBRARY_DEINIT ClassLibraryDeinitialize;
    PFN_CLASS_LIBRARY_BIND_CLIENT ClassLibraryBindClient;
    PFN_CLASS_LIBRARY_UNBIND_CLIENT ClassLibraryUnbindClient;
} WDF_CLASS_LIBRARY_INFO, *PWDF_CLASS_LIBRARY_INFO;

NTSTATUS
WdfVersionBindClass(
    _In_ PWDF_BIND_INFO BindInfo,
    _In_ PWDF_COMPONENT_GLOBALS Globals,
    _In_ PWDF_CLASS_BIND_INFO ClassBindInfo
    );

VOID
WdfVersionUnbindClass(
    _In_ PWDF_BIND_INFO BindInfo,
    _In_ PWDF_COMPONENT_GLOBALS Globals,
    _In_ PWDF_CLASS_BIND_INFO ClassBindInfo
    );

NTSTATUS
WdfRegisterClassLibrary(
    _In_ PWDF_CLASS_LIBRARY_INFO ClassLibraryInfo,
    _In_ PUNICODE_STRING SourceString,
    _In_ PUNICODE_STRING ObjectName
    );

NTSTATUS
WdfLdrQueryInterface(
    _In_ PWDF_INTERFACE_HEADER LoaderInterface
    );

#ifdef __cplusplus
}
#endif

#endif /* _TUR_WDFLDR_H_ */
