/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md).

   What WPP's preprocessor would generate for the framework's trace calls,
   without WPP: the trace flag names of fxtrace.h's control GUID as
   constants (those DbgTrace.h also defines keep its values) and the
   DoTraceLevelMessage function every trace call becomes
   (compat/tur-wdf-missing.cpp). Included at the end of ntddk.h
   (compat/ntddk.h), so that headers using a flag before a source file's
   .tmh see it. */
#ifndef _TUR_WDF_TRACE_H_
#define _TUR_WDF_TRACE_H_

#ifndef TRACINGDEVICE
#define TRACINGDEVICE         0x00000001
#define TRACINGOBJECT         0x00000002
#define TRACINGAPIERROR       0x00000004
#define TRACINGHANDLE         0x00000008
#define TRACINGPOOL           0x00000010
#define TRACINGERROR          0x00000020
#define TRACINGUSEROBJECT     0x00000040
#define TRACINGREQUEST        0x00000080
#define TRACINGIO             0x00000100
#define TRACINGPNP            0x00000200
#define TRACINGDRIVER         0x00001000
#define TRACINGPNPPOWERSTATES 0x00002000
#endif
#ifndef TRACINGFULL
#define TRACINGFULL           0x00010000
#define TRACINGDBGPRINT       0x00020000
#define TRACINGFRAMEWORKS     0x00040000
#define TRACINGAPI            0x00080000
#define TRACINGRESOURCES      0x00100000
#define TRACINGLOCKING        0x00200000
#define TRACINGCONTEXT        0x00400000
#define TRACINGIOTARGET       0x00800000
#define TRACINGDMA            0x01000000
#define TRACINGIFRCAPTURE     0x02000000
#endif

#ifndef TRACE_LEVEL_NONE
#define TRACE_LEVEL_NONE        0
#define TRACE_LEVEL_CRITICAL    1
#define TRACE_LEVEL_FATAL       1
#define TRACE_LEVEL_ERROR       2
#define TRACE_LEVEL_WARNING     3
#define TRACE_LEVEL_INFORMATION 4
#define TRACE_LEVEL_VERBOSE     5
#endif

#ifdef __cplusplus
extern "C" {
#endif
void __cdecl DoTraceLevelMessage(PVOID FxDriverGlobals, ULONG DebugPrintLevel, ULONG DebugPrintFlag,
                                 PCSTR DebugMessage, ...);
#ifdef __cplusplus
}
#endif

#ifndef WPP_INIT_TRACING
#define WPP_INIT_TRACING(DriverObject, RegistryPath)
#define WPP_CLEANUP(DriverObject)
#endif

#endif /* _TUR_WDF_TRACE_H_ */
