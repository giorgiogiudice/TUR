/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md).

   Wraps mingw-w64's usbdrivr.h, which includes its own usbdlib.h by a
   quoted name and so bypasses compat/usbdlib.h: includes it, then
   compat/usbdlib.h for the USB client-contract declarations. */
#include_next <usbdrivr.h>
#include <usbdlib.h>
