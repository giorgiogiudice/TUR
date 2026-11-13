/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md): a stand-in, written for this
   build, for the WDK's private ntwmi.h, which is not in Microsoft's
   MIT-licensed sources nor in mingw-w64: the WDF performance-trace
   callbacks, written from their use in globals.cpp and fxperftracekm.hpp.
   WmiQueryTraceInformation(WdfNotifyRoutinesClass) is expected to fail; the
   framework then uses an empty table and traces nothing. */
#pragma once

typedef
VOID
(NTAPI *PWMI_WDF_NOTIFY_ROUTINE)(
    _In_ PVOID EventData,
    _In_ ULONG EventDataSize,
    _In_ ULONG GroupMask,
    _In_ USHORT HookId,
    _In_ UCHAR Version
    );

typedef struct _WMI_WDF_NOTIFY_ROUTINES {
    ULONG Size;
    PWMI_WDF_NOTIFY_ROUTINE DpcNotifyRoutine;
    PWMI_WDF_NOTIFY_ROUTINE InterruptNotifyRoutine;
    PWMI_WDF_NOTIFY_ROUTINE WorkItemNotifyRoutine;
} WMI_WDF_NOTIFY_ROUTINES, *PWMI_WDF_NOTIFY_ROUTINES;

#define PERF_WDF_DPC        0x20000040
#define PERF_WDF_INTERRUPT  0x20000080

#define PERFINFO_LOG_TYPE_WDF_DPC               0x0d03
#define PERFINFO_LOG_TYPE_WDF_INTERRUPT         0x0d02
#define PERFINFO_LOG_TYPE_WDF_PASSIVE_INTERRUPT 0x0d04
#define PERFINFO_LOG_TYPE_WDF_WORK_ITEM         0x0d05
