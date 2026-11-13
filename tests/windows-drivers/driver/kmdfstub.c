// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// A stand-in for Microsoft's KMDF stub library (WdfDriverEntry.lib), which
// is not part of the MIT-licensed Windows-Driver-Frameworks sources: just
// enough for kmdf-fx2.sh to link Microsoft's kmdf_fx2 sample with mingw-w64.
// Written from the declarations in the framework's public headers (wdf.h,
// wdffuncenum.h) and its loader interface (fxldr.h).
//
// FxDriverEntry is the image's entry point, as in a KMDF driver built with
// the WDK: it binds to the framework through WDFLDR.SYS's WdfVersionBind and
// then calls the driver's DriverEntry. In Wine there is no WDFLDR.SYS, so the
// image is expected not to load at all; this file exists so that the attempt
// is made with the sample itself and not only with kmdfprobe.c.
#include <ddk/wdm.h>

#ifndef KMDF_VERSION_MINOR
#define KMDF_VERSION_MINOR 33
#endif

typedef void (*twd_wdffunc)(void);

typedef struct {
    ULONG Size;
    PWCHAR Component;
    struct { ULONG Major, Minor, Build; } Version;
    ULONG FuncCount;
    twd_wdffunc *FuncTable;
    void *Module;
} TWD_WDF_BIND_INFO;

__declspec(dllimport) NTSTATUS NTAPI WdfVersionBind(DRIVER_OBJECT *driver, UNICODE_STRING *registry_path,
                                                     TWD_WDF_BIND_INFO *bind_info, void **globals);

NTSTATUS NTAPI DriverEntry(DRIVER_OBJECT *driver, UNICODE_STRING *registry_path);

// The names the framework headers declare extern.
#define TWD_CAT2(a, b) a##b
#define TWD_CAT(a, b) TWD_CAT2(a, b)
#if KMDF_VERSION_MINOR < 10
#define TWD_TABLE TWD_CAT(WdfFunctions_0100, KMDF_VERSION_MINOR)
#else
#define TWD_TABLE TWD_CAT(WdfFunctions_010, KMDF_VERSION_MINOR)
#endif

static twd_wdffunc twd_functions[512];
twd_wdffunc *TWD_TABLE = twd_functions;
void *WdfDriverGlobals;
BOOLEAN WdfClientVersionHigherThanFramework;
ULONG WdfFunctionCount = 512;
void *WdfStructures;
ULONG WdfStructureCount;
BOOLEAN WdfMinimumVersionRequired;

NTSTATUS NTAPI FxDriverEntry(DRIVER_OBJECT *driver, UNICODE_STRING *registry_path)
{
    TWD_WDF_BIND_INFO info;
    void *globals = NULL;
    NTSTATUS status;

    RtlZeroMemory(&info, sizeof(info));
    info.Size = sizeof(info);
    info.Component = L"KmdfLibrary";
    info.Version.Major = 1;
    info.Version.Minor = KMDF_VERSION_MINOR;
    info.FuncCount = 512;
    info.FuncTable = twd_functions;
    DbgPrint("osrusbfx2: FxDriverEntry reached; calling WdfVersionBind\n");
    status = WdfVersionBind(driver, registry_path, &info, &globals);
    if (status < 0)
        return status;
    WdfDriverGlobals = globals;
    DbgPrint("osrusbfx2: DriverEntry\n");
    return DriverEntry(driver, registry_path);
}
