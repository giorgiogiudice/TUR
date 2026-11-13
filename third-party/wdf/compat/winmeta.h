/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md): a stand-in, written for this
   build, for the SDK's winmeta.h, which mingw-w64 does not ship: the
   standard ETW event levels and keywords (documented values). */
#pragma once

#define WINEVENT_LEVEL_LOG_ALWAYS 0x0
#define WINEVENT_LEVEL_CRITICAL   0x1
#define WINEVENT_LEVEL_ERROR      0x2
#define WINEVENT_LEVEL_WARNING    0x3
#define WINEVENT_LEVEL_INFO       0x4
#define WINEVENT_LEVEL_VERBOSE    0x5

#define WINEVENT_KEYWORD_ANY 0x0
