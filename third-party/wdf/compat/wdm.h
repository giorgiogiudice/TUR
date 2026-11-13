/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md).

   Wraps mingw-w64's wdm.h. On x86-64 mingw-w64 defines KeGetCurrentThread
   (and PsGetCurrentThread through it) as an inline read of gs:[0x188], the
   current thread in a real kernel's processor control region. In Wine's
   driver host gs points at the thread's TEB, where that offset is zero in
   every thread, so every thread looked like the same (NULL) thread: the
   framework's lock-owner tests (FxCallbackLock::IsOwner) then said "owner"
   for an unowned lock and an I/O queue re-queued its DPC forever (run
   43390973700). Call ntoskrnl's exports instead, as a WDK build with the
   real headers would on Wine. */
#define KeGetCurrentThread tur_mingw_gs_KeGetCurrentThread
#define PsGetCurrentThread tur_mingw_gs_PsGetCurrentThread
#include_next <wdm.h>
#undef KeGetCurrentThread
#undef PsGetCurrentThread

#ifndef _TUR_WDF_WDM_CURRENT_THREAD_
#define _TUR_WDF_WDM_CURRENT_THREAD_
#ifdef __cplusplus
extern "C" {
#endif
NTKERNELAPI PKTHREAD NTAPI KeGetCurrentThread(VOID);
NTKERNELAPI PETHREAD NTAPI PsGetCurrentThread(VOID);
#ifdef __cplusplus
}
#endif
#endif
