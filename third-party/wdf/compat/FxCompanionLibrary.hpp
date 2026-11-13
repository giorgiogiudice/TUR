/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md): a stand-in, written for this
   build, for the private FxCompanionLibrary.hpp (UMDF companion support in
   the kernel-mode framework), which is not in Microsoft's MIT-licensed
   sources. Written from its use in version.cpp and fxpkgpnp.cpp. No device
   ever needs a companion here. */
#pragma once

class FxDevice;

class FxCompanionLibrary {
public:
    void *operator new(size_t Size)
    {
        return ExAllocatePool2(POOL_FLAG_NON_PAGED, Size, 'LCuT');
    }

    void operator delete(void *Memory)
    {
        if (Memory != NULL) {
            ExFreePool(Memory);
        }
    }

    static NTSTATUS _CreateAndInitialize(FxCompanionLibrary **Library)
    {
        *Library = new FxCompanionLibrary();
        return *Library != NULL ? STATUS_SUCCESS : STATUS_INSUFFICIENT_RESOURCES;
    }

    BOOLEAN IsCompanionRequiredForDevice(FxDevice *Device, PCWSTR *CompanionName)
    {
        UNREFERENCED_PARAMETER(Device);
        *CompanionName = NULL;
        return FALSE;
    }
};
