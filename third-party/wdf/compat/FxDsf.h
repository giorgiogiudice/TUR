/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md): a stand-in, written for this
   build, for the private FxDsf.h, which is not in Microsoft's MIT-licensed
   sources. The Device Simulation Framework interface, written from its use
   in fxpkgfdokm.cpp. No lower driver in Wine offers it: the query fails and
   the framework uses the system's own interrupt routines. */
#pragma once

/* An arbitrary GUID: nothing in Wine answers a query for it. */
DEFINE_GUID(GUID_WDF_DSF_INTERFACE,
    0x8a5c7f9e, 0x6e1a, 0x4b54, 0x9c, 0x4e, 0x2f, 0x7a, 0x6d, 0x3b, 0x1c, 0x10);

#define WDM_DSF_INTERFACE_V1_0 0x100

typedef struct _WDF_DSF_INTERFACE {
    INTERFACE Interface;
    PFN_IO_CONNECT_INTERRUPT_EX IoConnectInterruptEx;
    PFN_IO_DISCONNECT_INTERRUPT_EX IoDisconnectInterruptEx;
    PFN_IO_REPORT_INTERRUPT_ACTIVE IoReportInterruptActive;
    PFN_IO_REPORT_INTERRUPT_INACTIVE IoReportInterruptInactive;
} WDF_DSF_INTERFACE, *PWDF_DSF_INTERFACE;
