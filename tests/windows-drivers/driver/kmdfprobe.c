// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// kmdfprobe.sys: a driver that binds to the Kernel-Mode Driver Framework the
// way every KMDF driver does, and nothing else.
//
// A KMDF driver is linked with Microsoft's stub library (WdfDriverEntry.lib).
// The stub's entry point calls WdfVersionBind, imported from WDFLDR.SYS, the
// framework loader; the loader loads the framework itself (Wdf01000.sys) and
// hands back the table of WDF functions the driver calls through. Both
// WDFLDR.SYS and Wdf01000.sys are Microsoft binaries. The stub is not part of
// Microsoft's MIT-licensed Windows-Driver-Frameworks sources, so this file
// stands in for it: it imports WdfVersionBind from WDFLDR.SYS exactly as the
// stub does (import library made from wdfldr.def) and calls it from
// DriverEntry.
//
// The question it answers: does Wine provide a WDFLDR.SYS a KMDF driver can
// bind to? If it does not, the image does not load and DriverEntry never
// runs; the report records Wine's own message.
//
// Like twdusb.sys it is only ever loaded by Wine's winedevice.exe, a Linux
// user-space process, never into any kernel.
#include <ddk/wdm.h>

// From WdfLdr's interface (fxldr.h in Windows-Driver-Frameworks): the shape
// is enough, the call is never expected to be reached.
typedef struct {
    ULONG Size;
    PWCHAR Component;
    struct { ULONG Major, Minor, Build; } Version;
    ULONG FuncCount;
    void *FuncTable;
    void *Module;
} TWD_WDF_BIND_INFO;

__declspec(dllimport) NTSTATUS NTAPI WdfVersionBind(DRIVER_OBJECT *driver, UNICODE_STRING *registry_path,
                                                     TWD_WDF_BIND_INFO *bind_info, void **globals);

static void *wdf_functions[512];

NTSTATUS NTAPI DriverEntry(DRIVER_OBJECT *driver, UNICODE_STRING *registry_path)
{
    TWD_WDF_BIND_INFO info;
    void *globals = NULL;
    NTSTATUS status;

    RtlZeroMemory(&info, sizeof(info));
    info.Size = sizeof(info);
    info.Component = L"KmdfLibrary";
    info.Version.Major = 1;
    info.Version.Minor = 15;
    info.FuncCount = 444;
    info.FuncTable = wdf_functions;
    DbgPrint("kmdfprobe: DriverEntry reached; calling WdfVersionBind\n");
    status = WdfVersionBind(driver, registry_path, &info, &globals);
    DbgPrint("kmdfprobe: WdfVersionBind returned %#lx\n", (ULONG)status);
    return status;
}
