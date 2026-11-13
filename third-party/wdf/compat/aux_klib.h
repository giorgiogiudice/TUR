/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md): a stand-in, written for this
   build, for the WDK's aux_klib.h (the header of a WDK static library),
   which is not in Microsoft's MIT-licensed sources nor in mingw-w64. Used
   only by the framework's bug-check callback (fxbugcheckcallback.cpp) to
   find which driver crashed; in a user-space driver host there is no bug
   check, so every call fails and the callback finds nothing. */
#pragma once

#define AUX_KLIB_MODULE_PATH_LEN 256

typedef struct _AUX_MODULE_BASIC_INFO {
    PVOID ImageBase;
} AUX_MODULE_BASIC_INFO, *PAUX_MODULE_BASIC_INFO;

typedef struct _AUX_MODULE_EXTENDED_INFO {
    AUX_MODULE_BASIC_INFO BasicInfo;
    ULONG ImageSize;
    USHORT FileNameOffset;
    UCHAR FullPathName[AUX_KLIB_MODULE_PATH_LEN];
} AUX_MODULE_EXTENDED_INFO, *PAUX_MODULE_EXTENDED_INFO;

typedef struct _KBUGCHECK_DATA {
    ULONG BugCheckDataSize;
    ULONG BugCheckCode;
    ULONG_PTR Parameter1;
    ULONG_PTR Parameter2;
    ULONG_PTR Parameter3;
    ULONG_PTR Parameter4;
} KBUGCHECK_DATA, *PKBUGCHECK_DATA;

static __inline NTSTATUS AuxKlibInitialize(VOID)
{
    return STATUS_NOT_SUPPORTED;
}

static __inline NTSTATUS AuxKlibQueryModuleInformation(PULONG BufferSize, ULONG ElementSize, PVOID QueryInfo)
{
    UNREFERENCED_PARAMETER(ElementSize);
    UNREFERENCED_PARAMETER(QueryInfo);
    if (BufferSize != NULL) {
        *BufferSize = 0;
    }
    return STATUS_NOT_SUPPORTED;
}

static __inline NTSTATUS AuxKlibGetBugCheckData(PKBUGCHECK_DATA BugCheckData)
{
    UNREFERENCED_PARAMETER(BugCheckData);
    return STATUS_NOT_SUPPORTED;
}
