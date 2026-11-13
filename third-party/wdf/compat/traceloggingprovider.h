/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md): a stand-in, written for this
   build, for the SDK's TraceLoggingProvider.h, which mingw-w64 does not
   ship. TraceLogging (and with it the framework's telemetry) is off: a
   provider is a null handle, registering it succeeds, it is never enabled
   and every write does nothing (its arguments are not evaluated). */
#pragma once

#include "winmeta.h"

typedef const struct _TlgProvider_t *TraceLoggingHProvider;

#define TRACELOGGING_DECLARE_PROVIDER(hProvider) \
    extern const TraceLoggingHProvider hProvider
#define TRACELOGGING_DEFINE_PROVIDER(hProvider, ...) \
    extern const TraceLoggingHProvider hProvider = NULL

#define TraceLoggingRegister(hProvider) ((void)(hProvider), (NTSTATUS)0)
#define TraceLoggingRegisterEx(hProvider, ...) ((void)(hProvider), (NTSTATUS)0)
#define TraceLoggingUnregister(hProvider) ((void)(hProvider))
#define TraceLoggingProviderEnabled(hProvider, level, keyword) ((void)(hProvider), FALSE)
#define TraceLoggingWrite(hProvider, eventName, ...) ((void)(hProvider))
#define TraceLoggingWriteActivity(hProvider, eventName, ...) ((void)(hProvider))
#define TraceLoggingOptionGroup(...)
