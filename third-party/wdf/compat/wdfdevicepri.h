/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md): a stand-in, written for this
   build, for the WDK's private wdfdevicepri.h, which is not in Microsoft's
   MIT-licensed sources: the self I/O target DDIs, prototypes as in
   fxdynamics.h. */
#ifndef _TUR_WDFDEVICEPRI_H_
#define _TUR_WDFDEVICEPRI_H_

WDF_EXTERN_C_START

typedef WDFAPI WDFIOTARGET (NTAPI *PFN_WDFDEVICEGETSELFIOTARGET)(
    _In_ PWDF_DRIVER_GLOBALS DriverGlobals, _In_ WDFDEVICE Device);
typedef WDFAPI VOID (NTAPI *PFN_WDFDEVICEINITALLOWSELFIOTARGET)(
    _In_ PWDF_DRIVER_GLOBALS DriverGlobals, _In_ PWDFDEVICE_INIT DeviceInit);

WDF_EXTERN_C_END

#endif /* _TUR_WDFDEVICEPRI_H_ */
