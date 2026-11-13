// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// A stand-in for the WDK's KMDF stub library (WdfDriverEntry.lib, a
// Microsoft binary that is not in the MIT-licensed Windows-Driver-Frameworks
// sources), written from the loader interface in those sources (fxldr.h,
// fxlibrarycommon.cpp) and the globals the public KMDF headers declare
// (wdf.h, wdfglobals.h, wdffuncenum.h). Linked into each KMDF driver this
// experiment builds (the kmdf_fx2 sample, kmdftest); FxDriverEntry is the
// image's entry point.
//
// What a KMDF driver's start-up does, and this does:
//   1. bind to the framework: WdfVersionBind(), imported from WDFLDR.SYS,
//      with the driver's version and a pointer to its function-table
//      variable; the framework stores its table there (KMDF 1.15 and later
//      tables are handed over by pointer, fxlibrarycommon.cpp) and returns
//      the driver's globals;
//   2. call the driver's own DriverEntry;
//   3. unbind when the driver is unloaded, after the driver's own unload.
#include <ntddk.h>
#include <wdf.h>

typedef struct {
    ULONG Major, Minor, Build;
} TUR_WDF_VERSION;

typedef struct {
    ULONG Size;
    PWCHAR Component;
    TUR_WDF_VERSION Version;
    ULONG FuncCount;
    WDFFUNC *FuncTable;
    PVOID Module;
} TUR_WDF_BIND_INFO;

__declspec(dllimport) NTSTATUS NTAPI WdfVersionBind(PDRIVER_OBJECT DriverObject, PUNICODE_STRING RegistryPath,
                                                   TUR_WDF_BIND_INFO *BindInfo, PWDF_DRIVER_GLOBALS *Globals);
__declspec(dllimport) NTSTATUS NTAPI WdfVersionUnbind(PUNICODE_STRING RegistryPath, TUR_WDF_BIND_INFO *BindInfo,
                                                     PWDF_DRIVER_GLOBALS Globals);

DRIVER_INITIALIZE DriverEntry;

// The globals the public headers declare extern (wdf.h maps WdfFunctions to
// WdfFunctions_010<minor>).
const WDFFUNC *WdfFunctions = NULL;
WDFFUNC WdfDriverMiniportUnloadOverride = NULL;
PWDF_DRIVER_GLOBALS WdfDriverGlobals = NULL;
BOOLEAN WdfClientVersionHigherThanFramework = FALSE;
ULONG WdfFunctionCount = 0;
ULONG WdfStructureCount = 0;
WDF_STRUCT_INFO WdfStructures = NULL;

static TUR_WDF_BIND_INFO g_BindInfo;
static UNICODE_STRING g_RegistryPath;
static WCHAR g_RegistryPathBuffer[260];
static PDRIVER_UNLOAD g_ClientUnload;

static VOID NTAPI FxStubDriverUnload(PDRIVER_OBJECT DriverObject)
{
    if (g_ClientUnload != NULL) {
        g_ClientUnload(DriverObject);
    }
    (VOID) WdfVersionUnbind(&g_RegistryPath, &g_BindInfo, WdfDriverGlobals);
}

NTSTATUS NTAPI FxDriverEntry(PDRIVER_OBJECT DriverObject, PUNICODE_STRING RegistryPath)
{
    NTSTATUS status;

    g_RegistryPath.Buffer = g_RegistryPathBuffer;
    g_RegistryPath.Length = 0;
    g_RegistryPath.MaximumLength = sizeof(g_RegistryPathBuffer);
    if (RegistryPath != NULL) {
        RtlCopyUnicodeString(&g_RegistryPath, RegistryPath);
    }

    RtlZeroMemory(&g_BindInfo, sizeof(g_BindInfo));
    g_BindInfo.Size = sizeof(g_BindInfo);
    g_BindInfo.Component = L"KmdfLibrary";
    g_BindInfo.Version.Major = KMDF_VERSION_MAJOR;
    g_BindInfo.Version.Minor = KMDF_VERSION_MINOR;
    g_BindInfo.FuncCount = WdfFunctionTableNumEntries;
    g_BindInfo.FuncTable = (WDFFUNC *) &WdfFunctions;

    DbgPrint("TUR fxstub: binding to KMDF %lu.%lu, %lu functions\n", (ULONG) KMDF_VERSION_MAJOR,
             (ULONG) KMDF_VERSION_MINOR, (ULONG) WdfFunctionTableNumEntries);
    status = WdfVersionBind(DriverObject, &g_RegistryPath, &g_BindInfo, &WdfDriverGlobals);
    DbgPrint("TUR fxstub: WdfVersionBind returned 0x%08lx, globals %p, table %p\n", (ULONG) status,
             WdfDriverGlobals, WdfFunctions);
    if (!NT_SUCCESS(status)) {
        return status;
    }
    WdfFunctionCount = WdfFunctionTableNumEntries;
    WdfStructureCount = WDF_STRUCTURE_TABLE_NUM_ENTRIES;

    status = DriverEntry(DriverObject, RegistryPath);
    DbgPrint("TUR fxstub: DriverEntry returned 0x%08lx\n", (ULONG) status);
    if (!NT_SUCCESS(status)) {
        (VOID) WdfVersionUnbind(&g_RegistryPath, &g_BindInfo, WdfDriverGlobals);
        return status;
    }
    if (DriverObject->DriverUnload != NULL) {
        g_ClientUnload = DriverObject->DriverUnload;
        DriverObject->DriverUnload = FxStubDriverUnload;
    }
    return status;
}
