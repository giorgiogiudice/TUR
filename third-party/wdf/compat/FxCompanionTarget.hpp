/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md): a stand-in, written for this
   build, for the private FxCompanionTarget.hpp (the UMDF companion target
   object), which is not in Microsoft's MIT-licensed sources. Written from its
   use in fxdevicekm.cpp and fxpkgpnp.cpp. FxCompanionLibrary never asks for
   one, so none is ever created; Init would fail if one were. */
#pragma once

class FxCompanionTarget : public FxNonPagedObject {
public:
    FxCompanionTarget(PFX_DRIVER_GLOBALS FxDriverGlobals, USHORT ObjectSize) :
        FxNonPagedObject(FX_TYPE_COMPANION_TARGET, ObjectSize, FxDriverGlobals)
    {
    }

    NTSTATUS Init(FxDevice *Device)
    {
        UNREFERENCED_PARAMETER(Device);
        return STATUS_NOT_SUPPORTED;
    }

    VOID QueryPnPDeviceStateNotification(VOID)
    {
    }

    NTSTATUS HandleQueryInterfaceForSecureDriver(FxIrp *Irp, PBOOLEAN CompleteRequest)
    {
        *CompleteRequest = FALSE;
        return Irp->GetStatus();
    }
};
