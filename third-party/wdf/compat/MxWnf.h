/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md): a stand-in, written for this
   build, for the private MxWnf.h, which is not in Microsoft's MIT-licensed
   sources. Windows Notification Facility subscriptions, used only for
   sleep-study reporting; written from their use in fxpkgpnpkm.cpp. Every call
   fails, which the framework treats as "sleep study disabled". */
#pragma once

#ifndef _TUR_WNF_STATE_NAME_DEFINED
#define _TUR_WNF_STATE_NAME_DEFINED
typedef struct _WNF_STATE_NAME {
    ULONG Data[2];
} WNF_STATE_NAME, *PWNF_STATE_NAME;
typedef const WNF_STATE_NAME *PCWNF_STATE_NAME;
#endif

/* Any value: the subscription is never made. */
#define WNF_PO_DRIPS_DEVICE_CONSTRAINTS_REGISTERED { 0xa3bc0875, 0x41940b3a }

typedef struct _MxWnfSubscriptionContext *PMxWnfSubscriptionContext;

typedef NTSTATUS (*PFN_MX_WNF_CALLBACK)(PMxWnfSubscriptionContext SubscriptionContext, PVOID CallbackContext);

class MxWnf {
public:
    static NTSTATUS MxSubscribeWnfStateChange(PMxWnfSubscriptionContext *SubscriptionContext,
                                              PCWNF_STATE_NAME StateName,
                                              PFN_MX_WNF_CALLBACK Callback,
                                              PVOID CallbackContext,
                                              PVOID Tag)
    {
        UNREFERENCED_PARAMETER(StateName);
        UNREFERENCED_PARAMETER(Callback);
        UNREFERENCED_PARAMETER(CallbackContext);
        UNREFERENCED_PARAMETER(Tag);
        *SubscriptionContext = NULL;
        return STATUS_NOT_SUPPORTED;
    }

    static NTSTATUS MxQueryWnfStateData(PMxWnfSubscriptionContext SubscriptionContext, PVOID Buffer,
                                        PULONG BufferSize)
    {
        UNREFERENCED_PARAMETER(SubscriptionContext);
        UNREFERENCED_PARAMETER(Buffer);
        UNREFERENCED_PARAMETER(BufferSize);
        return STATUS_NOT_SUPPORTED;
    }

    static VOID MxUnsubscribeWnfStateChange(PMxWnfSubscriptionContext *SubscriptionContext)
    {
        *SubscriptionContext = NULL;
    }
};
