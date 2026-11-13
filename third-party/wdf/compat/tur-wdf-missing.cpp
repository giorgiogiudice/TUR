/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md).

   Functions the framework calls that are not in Microsoft's MIT-licensed
   sources (they live in WDK libraries or in files that were not published),
   written for this build from their declarations and their use in the
   sources:

     WdfBindClientHelper                 fxldr.h; version check at binding
     WdfLdrDiagnosticsValueByNameAsULONG wdfldr.h; the loader's diagnostics key
     DoTraceLevelMessage                 DbgTrace.h; replaces dbgtrace.cpp, see below
     imp_WdfCompanionTarget*             fxdynamics.h; UMDF companions: not supported
     FxVerifierQueryStateSeparationDetection  globals.cpp; verifier option, "none"
*/
#include "fx.hpp"
#include "fxldr.h"

extern "C" {

/* The framework accepts a client built for its major version and for the
   same or an older minor version; a client built for a newer minor version
   binds only if it says (bind info v2) that it can run on this one. */
NTSTATUS
WdfBindClientHelper(
    _Inout_ PWDF_BIND_INFO BindInfo,
    _In_ WDF_MAJOR_VERSION FxMajorVersion,
    _In_ WDF_MINOR_VERSION FxMinorVersion
    )
{
    PWDF_BIND_INFO2 info2 = NULL;

    if (BindInfo->Version.Major != FxMajorVersion) {
        return STATUS_OBJECT_TYPE_MISMATCH;
    }
    if (BindInfo->Size >= sizeof(WDF_BIND_INFO2)) {
        info2 = (PWDF_BIND_INFO2) BindInfo;
    }
    if (BindInfo->Version.Minor <= FxMinorVersion) {
        if (info2 != NULL && info2->ClientVersionHigherThanFramework != NULL) {
            *info2->ClientVersionHigherThanFramework = FALSE;
        }
        return STATUS_SUCCESS;
    }
    if (info2 != NULL && info2->MinimumVersionRequired != NULL &&
        *info2->MinimumVersionRequired <= FxMinorVersion) {
        if (info2->ClientVersionHigherThanFramework != NULL) {
            *info2->ClientVersionHigherThanFramework = TRUE;
        }
        return STATUS_SUCCESS;
    }
    return STATUS_OBJECT_TYPE_MISMATCH;
}

/* HKLM\System\CurrentControlSet\Control\Wdf\Kmdf\Diagnostics\<ValueName> */
NTSTATUS
WdfLdrDiagnosticsValueByNameAsULONG(
    _In_ PUNICODE_STRING ValueName,
    _Out_ PULONG Value
    )
{
    DECLARE_CONST_UNICODE_STRING(keyName,
        L"\\Registry\\Machine\\System\\CurrentControlSet\\Control\\Wdf\\Kmdf\\Diagnostics");
    UCHAR buffer[FIELD_OFFSET(KEY_VALUE_PARTIAL_INFORMATION, Data) + sizeof(ULONG)];
    PKEY_VALUE_PARTIAL_INFORMATION info = (PKEY_VALUE_PARTIAL_INFORMATION) buffer;
    OBJECT_ATTRIBUTES attributes;
    HANDLE key;
    ULONG length;
    NTSTATUS status;

    InitializeObjectAttributes(&attributes, (PUNICODE_STRING) &keyName,
                               OBJ_CASE_INSENSITIVE | OBJ_KERNEL_HANDLE, NULL, NULL);
    status = ZwOpenKey(&key, KEY_QUERY_VALUE, &attributes);
    if (!NT_SUCCESS(status)) {
        return status;
    }
    status = ZwQueryValueKey(key, ValueName, KeyValuePartialInformation, buffer, sizeof(buffer), &length);
    ZwClose(key);
    if (!NT_SUCCESS(status)) {
        return status;
    }
    if (info->Type != REG_DWORD || info->DataLength != sizeof(ULONG)) {
        return STATUS_OBJECT_TYPE_MISMATCH;
    }
    RtlCopyMemory(Value, info->Data, sizeof(ULONG));
    return STATUS_SUCCESS;
}

/* The framework's trace messages are WPP format strings: printf with extra
   %!NAME! conversions (%!STATUS!, %!GUID!, %!WDF_POWER_DEVICE_STATE!, ...)
   that WPP's preprocessor turns into binary trace records. The sources'
   own DbgTrace.h fallback (dbgtrace.cpp) prints them with a plain printf
   only in checked builds, where the %!NAME! conversions would misread the
   arguments. This replacement translates them -- %!GUID! to %p, %!FUNC!,
   %!LINE! and %!FILE! to nothing, every other %!NAME! (a status, an
   enumeration value or a flag set, all 32-bit) to 0x%x -- and prints
   through DbgPrintEx, which Wine shows on its "debugstr" channel
   (WINEDEBUG=warn+debugstr). Messages up to TRACE_LEVEL_INFORMATION are
   printed; the level can be changed with the loader's diagnostics value
   TurTraceLevel. */
static ULONG g_TurTraceLevel = TRACE_LEVEL_INFORMATION;
static LONG g_TurTraceLevelRead = 0;

static VOID
TurTranslateWppFormat(
    _In_ PCSTR Format,
    _Out_writes_(Size) PCHAR Out,
    _In_ SIZE_T Size
    )
{
    SIZE_T o = 0;
    PCSTR p = Format;

    while (*p != '\0' && o + 8 < Size) {
        if (p[0] == '%' && p[1] == '!') {
            PCSTR end = p + 2;
            CHAR name[40];
            SIZE_T n = 0;
            PCSTR repl;

            while (*end != '\0' && *end != '!' && n + 1 < sizeof(name)) {
                name[n++] = *end++;
            }
            name[n] = '\0';
            if (*end != '!') {
                Out[o++] = *p++;
                continue;
            }
            if (strcmp(name, "GUID") == 0) {
                repl = "%p";
            }
            else if (strcmp(name, "FUNC") == 0 || strcmp(name, "LINE") == 0 ||
                     strcmp(name, "FILE") == 0 || strcmp(name, "LEVEL") == 0 ||
                     strcmp(name, "FLAGS") == 0 || strcmp(name, "COMPNAME") == 0) {
                repl = "";
            }
            else {
                repl = "0x%x";
            }
            while (*repl != '\0' && o + 1 < Size) {
                Out[o++] = *repl++;
            }
            p = end + 1;
            continue;
        }
        Out[o++] = *p++;
    }
    Out[o] = '\0';
}

VOID
__cdecl
DoTraceLevelMessage(
    __in PVOID FxDriverGlobals,
    __in ULONG DebugPrintLevel,
    __in ULONG DebugPrintFlag,
    __in PCSTR DebugMessage,
    ...
    )
{
    CHAR format[512];
    CHAR message[1024];
    va_list args;

    UNREFERENCED_PARAMETER(FxDriverGlobals);
    UNREFERENCED_PARAMETER(DebugPrintFlag);

    if (InterlockedCompareExchange(&g_TurTraceLevelRead, 1, 0) == 0 &&
        KeGetCurrentIrql() == PASSIVE_LEVEL) {
        DECLARE_CONST_UNICODE_STRING(name, L"TurTraceLevel");
        ULONG level;

        if (NT_SUCCESS(WdfLdrDiagnosticsValueByNameAsULONG((PUNICODE_STRING) &name, &level))) {
            g_TurTraceLevel = level;
        }
    }
    if (DebugMessage == NULL || DebugPrintLevel > g_TurTraceLevel) {
        return;
    }
    TurTranslateWppFormat(DebugMessage, format, sizeof(format));
    va_start(args, DebugMessage);
    (VOID) RtlStringCbVPrintfA(message, sizeof(message), format, args);
    va_end(args);
    DbgPrintEx(DPFLTR_IHVDRIVER_ID, DPFLTR_ERROR_LEVEL, "WDF[%lu]: %s\n", DebugPrintLevel, message);
}

/* UMDF companion targets do not exist in this build (see
   FxCompanionLibrary.hpp); their two DDIs fail. */
_Must_inspect_result_
NTSTATUS
NTAPI
WDFEXPORT(WdfCompanionTargetSendTaskSynchronously)(
    _In_ PWDF_DRIVER_GLOBALS DriverGlobals,
    _In_ WDFCOMPANIONTARGET CompanionTarget,
    _In_ USHORT TaskQueueIdentifier,
    _In_ ULONG TaskOperationCode,
    _In_opt_ PWDF_MEMORY_DESCRIPTOR InputBuffer,
    _In_opt_ PWDF_MEMORY_DESCRIPTOR OutputBuffer,
    _In_opt_ PWDF_TASK_SEND_OPTIONS TaskOptions,
    _Out_ PULONG_PTR BytesReturned
    )
{
    UNREFERENCED_PARAMETER(DriverGlobals);
    UNREFERENCED_PARAMETER(CompanionTarget);
    UNREFERENCED_PARAMETER(TaskQueueIdentifier);
    UNREFERENCED_PARAMETER(TaskOperationCode);
    UNREFERENCED_PARAMETER(InputBuffer);
    UNREFERENCED_PARAMETER(OutputBuffer);
    UNREFERENCED_PARAMETER(TaskOptions);
    if (BytesReturned != NULL) {
        *BytesReturned = 0;
    }
    return STATUS_NOT_SUPPORTED;
}

_Must_inspect_result_
PEPROCESS
NTAPI
WDFEXPORT(WdfCompanionTargetWdmGetCompanionProcess)(
    _In_ PWDF_DRIVER_GLOBALS DriverGlobals,
    _In_ WDFCOMPANIONTARGET CompanionTarget
    )
{
    UNREFERENCED_PARAMETER(DriverGlobals);
    UNREFERENCED_PARAMETER(CompanionTarget);
    return NULL;
}

} /* extern "C" */

/* globals.cpp declares this and calls it; its definition is not in the
   published sources. The only option the sources name is "none". */
VOID
FxVerifierQueryStateSeparationDetection(
    _In_ HANDLE Key,
    _Out_ FxStateSeparationDetectionOption* StateSeparationDetection
    )
{
    UNREFERENCED_PARAMETER(Key);
    *StateSeparationDetection = FxStateSeparationDetectionNone;
}
