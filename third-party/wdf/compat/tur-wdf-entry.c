/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md).

   The image entry point of Wdf01000.sys in the TUR build. The WDK's
   compiler runs a driver's C++ static constructors from its own start-up
   code (the GsDriverEntry stub); this build links without any start-up code,
   so this entry point runs them -- the list lld builds in MinGW mode,
   __CTOR_LIST__: a -1 or a count, the constructors, then a NULL, run from
   last to first as MinGW's own start-up code does -- and then calls the
   framework's DriverEntry (version.cpp). */
#include <ntddk.h>

typedef void (*tur_ctor)(void);
extern tur_ctor __CTOR_LIST__[];

NTSTATUS NTAPI DriverEntry(PDRIVER_OBJECT DriverObject, PUNICODE_STRING RegistryPath);

NTSTATUS NTAPI TurWdfEntry(PDRIVER_OBJECT DriverObject, PUNICODE_STRING RegistryPath)
{
    static LONG done;
    ULONG_PTR count = 0;

    if (InterlockedExchange(&done, 1) == 0) {
        while (__CTOR_LIST__[count + 1] != NULL) {
            count++;
        }
        while (count > 0) {
            __CTOR_LIST__[count--]();
        }
    }
    return DriverEntry(DriverObject, RegistryPath);
}

/* A pure virtual function called through an incomplete object: never in a
   correct program. The Itanium C++ ABI clang uses for MinGW names the
   handler this; the framework built with MSVC has its own. */
void __cxa_pure_virtual(void)
{
    KeBugCheckEx(0x000000E4 /* WORKER_INVALID */, 0x54555201, 0, 0, 0);
}
