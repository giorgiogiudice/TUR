/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md).

   Kernel declarations the framework uses that mingw-w64's DDK headers
   (14.0.0) lack, written from Microsoft's public documentation of each
   function and constant. Included at the end of ntddk.h (compat/ntddk.h).
   Each block is guarded so that a newer mingw-w64 that has the declaration
   wins. */
#pragma once

#include <ntstrsafe.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ExAllocatePool2 and its flags (Windows 10 2004). */
#ifndef POOL_FLAG_REQUIRED_START
typedef ULONG64 POOL_FLAGS;
#define POOL_FLAG_REQUIRED_START    0x0000000000000001ULL
#define POOL_FLAG_USE_QUOTA         0x0000000000000001ULL
#define POOL_FLAG_UNINITIALIZED     0x0000000000000002ULL
#define POOL_FLAG_SESSION           0x0000000000000004ULL
#define POOL_FLAG_CACHE_ALIGNED     0x0000000000000008ULL
#define POOL_FLAG_RAISE_ON_FAILURE  0x0000000000000020ULL
#define POOL_FLAG_NON_PAGED         0x0000000000000040ULL
#define POOL_FLAG_NON_PAGED_EXECUTE 0x0000000000000080ULL
#define POOL_FLAG_PAGED             0x0000000000000100ULL
#define POOL_FLAG_REQUIRED_END      0x0000000080000000ULL
NTKERNELAPI PVOID NTAPI ExAllocatePool2(_In_ POOL_FLAGS Flags, _In_ SIZE_T NumberOfBytes, _In_ ULONG Tag);
#endif

/* ExInitializeDriverRuntime(DrvRtPoolNxOptIn): makes NonPagedPool mean
   NonPagedPoolNx; a user-space driver host has no executable pool to
   avoid, so it does nothing here. */
#ifndef DrvRtPoolNxOptIn
#define DrvRtPoolNxOptIn 0x00000001
#define ExInitializeDriverRuntime(RuntimeFlags) ((void)(RuntimeFlags))
#endif

/* ntstrsafe.h's UNICODE_STRING functions, as documented. */
#ifndef _TUR_RTL_UNICODE_STRING_SAFE
#define _TUR_RTL_UNICODE_STRING_SAFE
static __inline NTSTATUS RtlUnicodeStringPrintf(PUNICODE_STRING DestinationString, NTSTRSAFE_PCWSTR Format, ...)
{
    NTSTATUS status;
    va_list args;
    size_t length = 0;

    if (DestinationString == NULL || DestinationString->Buffer == NULL || DestinationString->MaximumLength == 0) {
        return STATUS_INVALID_PARAMETER;
    }
    va_start(args, Format);
    status = RtlStringCbVPrintfW(DestinationString->Buffer, DestinationString->MaximumLength, Format, args);
    va_end(args);
    if (NT_SUCCESS(status) || status == STATUS_BUFFER_OVERFLOW) {
        while (length * sizeof(WCHAR) < DestinationString->MaximumLength && DestinationString->Buffer[length]) {
            length++;
        }
        DestinationString->Length = (USHORT)(length * sizeof(WCHAR));
    }
    return status;
}

static __inline NTSTATUS RtlUnicodeStringCopy(PUNICODE_STRING DestinationString, PCUNICODE_STRING SourceString)
{
    if (DestinationString == NULL || SourceString == NULL) {
        return STATUS_INVALID_PARAMETER;
    }
    if (SourceString->Length > DestinationString->MaximumLength) {
        RtlCopyMemory(DestinationString->Buffer, SourceString->Buffer, DestinationString->MaximumLength);
        DestinationString->Length = DestinationString->MaximumLength;
        return STATUS_BUFFER_OVERFLOW;
    }
    RtlCopyMemory(DestinationString->Buffer, SourceString->Buffer, SourceString->Length);
    DestinationString->Length = SourceString->Length;
    return STATUS_SUCCESS;
}
#endif

#ifdef __cplusplus
}
#endif
