/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../../README.md): a stand-in, written for
   this build, for Microsoft's internal telemetry-assert header, which is not
   in Microsoft's MIT-licensed sources. Telemetry asserts do nothing. */
#pragma once

#include "MicrosoftTelemetry.h"

#define MICROSOFT_TELEMETRY_ASSERT(x)
#define MICROSOFT_TELEMETRY_ASSERT_KM(x)
#define MICROSOFT_TELEMETRY_ASSERT_MSG(x, m)
#define MICROSOFT_TELEMETRY_ASSERT_MSG_KM(x, m)
#define InitializeTelemetryAssertsKMByName(...) ((NTSTATUS)0)
#define InitializeTelemetryAssertsKM(...) ((NTSTATUS)0)
#define UninitializeTelemetryAssertsKM(...) ((void)0)
