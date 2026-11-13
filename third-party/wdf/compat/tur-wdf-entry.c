/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md).

   The image entry point of Wdf01000.sys in the TUR build. The WDK's
   start-up code for a driver (GsDriverEntry) also runs C++ static
   constructors; the framework has none (link-wdf.sh checks every object for
   a .ctors section and reports any), so this entry point only calls the
   framework's DriverEntry (version.cpp). */
#include <ntddk.h>

NTSTATUS NTAPI DriverEntry(PDRIVER_OBJECT DriverObject, PUNICODE_STRING RegistryPath);

NTSTATUS NTAPI TurWdfEntry(PDRIVER_OBJECT DriverObject, PUNICODE_STRING RegistryPath)
{
    return DriverEntry(DriverObject, RegistryPath);
}

/* A pure virtual function called through an incomplete object: never in a
   correct program. The Itanium C++ ABI clang uses for MinGW names the
   handler this; the framework built with MSVC has its own. */
void __cxa_pure_virtual(void)
{
    KeBugCheckEx(0x000000E4 /* WORKER_INVALID */, 0x54555201, 0, 0, 0);
}
