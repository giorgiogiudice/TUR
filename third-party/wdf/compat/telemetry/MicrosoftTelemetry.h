/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../../README.md): a stand-in, written for
   this build, for Microsoft's internal telemetry header, which is not in
   Microsoft's MIT-licensed sources. Telemetry is off (see
   traceloggingprovider.h); these names only need to exist. Included as
   telemetry/MicrosoftTelemetry.h and telemetry/microsofttelemetry.h (the
   include map links the second spelling here). */
#pragma once

#include "traceloggingprovider.h"

#define TraceLoggingOptionMicrosoftTelemetry()
#define MICROSOFT_KEYWORD_CRITICAL_DATA 0x0000800000000000ULL
#define MICROSOFT_KEYWORD_MEASURES      0x0000400000000000ULL
#define MICROSOFT_KEYWORD_TELEMETRY     0x0000200000000000ULL
#define TelemetryPrivacyDataTag(tag)
#define PDT_BrowsingHistory                    0
#define PDT_DeviceConnectivityAndConfiguration 0
#define PDT_InkingTypingAndSpeechUtterance     0
#define PDT_ProductAndServicePerformance       0
#define PDT_ProductAndServiceUsage             0
#define PDT_SoftwareSetupAndInventory          0
