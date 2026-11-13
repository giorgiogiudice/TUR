/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md): a stand-in, written for this
   build, for SleepStudyHelper.h (the header of a WDK static library), which
   is not in Microsoft's MIT-licensed sources. Sleep-study reporting is not
   available: every call fails or does nothing. Written from the calls in
   fxpkgpnpkm.cpp and fxpkgpnp.hpp. */
#pragma once

typedef PVOID SS_LIBRARY, *PSS_LIBRARY;
typedef PVOID SS_COMPONENT, *PSS_COMPONENT;

typedef enum _SSH_GUID_TYPE {
    SSH_PDO = 1,
    SSH_FDO = 2,
} SSH_GUID_TYPE;

__inline NTSTATUS SleepstudyHelper_Initialize(PSS_LIBRARY Library, PVOID Context)
{
    UNREFERENCED_PARAMETER(Context);
    *Library = NULL;
    return STATUS_NOT_SUPPORTED;
}

__inline VOID SleepstudyHelper_Uninitialize(SS_LIBRARY Library)
{
    UNREFERENCED_PARAMETER(Library);
}

__inline VOID SleepstudyHelper_GenerateGuid(SSH_GUID_TYPE Type, ULONG64 Value, GUID *Guid)
{
    UNREFERENCED_PARAMETER(Type);
    RtlZeroMemory(Guid, sizeof(*Guid));
    RtlCopyMemory(Guid, &Value, sizeof(Value));
}

__inline NTSTATUS SleepstudyHelper_GetPdoFriendlyName(PDEVICE_OBJECT Pdo, PUNICODE_STRING Name)
{
    UNREFERENCED_PARAMETER(Pdo);
    UNREFERENCED_PARAMETER(Name);
    return STATUS_NOT_SUPPORTED;
}

__inline NTSTATUS SleepstudyHelper_RegisterComponentEx(SS_LIBRARY Library, GUID Parent, GUID Self,
                                                       PUNICODE_STRING FriendlyName, PSS_COMPONENT Component)
{
    UNREFERENCED_PARAMETER(Library);
    UNREFERENCED_PARAMETER(Parent);
    UNREFERENCED_PARAMETER(Self);
    UNREFERENCED_PARAMETER(FriendlyName);
    *Component = NULL;
    return STATUS_NOT_SUPPORTED;
}

__inline VOID SleepstudyHelper_UnregisterComponent(SS_COMPONENT Component)
{
    UNREFERENCED_PARAMETER(Component);
}

__inline VOID SleepstudyHelper_AcquireComponentLock(SS_COMPONENT Component, PKIRQL Irql)
{
    UNREFERENCED_PARAMETER(Component);
    *Irql = PASSIVE_LEVEL;
}

__inline VOID SleepstudyHelper_ReleaseComponentLock(SS_COMPONENT Component, KIRQL Irql)
{
    UNREFERENCED_PARAMETER(Component);
    UNREFERENCED_PARAMETER(Irql);
}

__inline VOID SleepstudyHelper_ComponentActive(SS_COMPONENT Component)
{
    UNREFERENCED_PARAMETER(Component);
}

__inline VOID SleepstudyHelper_ComponentActiveLocked(SS_COMPONENT Component)
{
    UNREFERENCED_PARAMETER(Component);
}

__inline VOID SleepstudyHelper_ComponentInactive(SS_COMPONENT Component)
{
    UNREFERENCED_PARAMETER(Component);
}

__inline VOID SleepstudyHelper_ResetComponentsStartTime(SS_COMPONENT Component)
{
    UNREFERENCED_PARAMETER(Component);
}
