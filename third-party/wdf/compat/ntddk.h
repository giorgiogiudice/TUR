/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md).

   Wraps mingw-w64's ntddk.h: includes it, then declares what the framework
   uses from the current WDK that mingw-w64's DDK headers lack
   (tur-ddk-additions.h). */
#include_next <ntddk.h>

#ifndef _TUR_WDF_NTDDK_ADDITIONS_
#define _TUR_WDF_NTDDK_ADDITIONS_
#include "tur-ddk-additions.h"
#endif
