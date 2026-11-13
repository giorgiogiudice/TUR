/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md): a stand-in, written for this
   build, for the WDK's private wdfiotargetpri.h, which is not in Microsoft's
   MIT-licensed sources: prototype as in fxdynamics.h. */
#ifndef _TUR_WDFIOTARGETPRI_H_
#define _TUR_WDFIOTARGETPRI_H_

WDF_EXTERN_C_START

typedef WDFAPI NTSTATUS (NTAPI *PFN_WDFIOTARGETSELFASSIGNDEFAULTIOQUEUE)(
    _In_ PWDF_DRIVER_GLOBALS DriverGlobals, _In_ WDFIOTARGET IoTarget, _In_ WDFQUEUE Queue);

WDF_EXTERN_C_END

#endif /* _TUR_WDFIOTARGETPRI_H_ */
