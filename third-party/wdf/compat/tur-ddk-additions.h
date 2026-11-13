/* SPDX-License-Identifier: MIT */
/* Copyright (c) 2026 TUR contributors. Part of the TUR build of Microsoft's
   Windows-Driver-Frameworks (see ../README.md).

   Kernel declarations the framework uses that mingw-w64's DDK headers
   (14.0.0) lack, written from Microsoft's public documentation of each
   function and constant. Included at the end of ntddk.h (compat/ntddk.h).
   Each block is guarded so that a newer mingw-w64 that has the declaration
   wins. */
#pragma once

#include <ntstrsafe.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ExAllocatePool2 and its flags (Windows 10 2004). */
#ifndef POOL_FLAG_REQUIRED_START
typedef ULONG64 POOL_FLAGS;
#define POOL_FLAG_REQUIRED_START    0x0000000000000001ULL
#define POOL_FLAG_USE_QUOTA         0x0000000000000001ULL
#define POOL_FLAG_UNINITIALIZED     0x0000000000000002ULL
#define POOL_FLAG_SESSION           0x0000000000000004ULL
#define POOL_FLAG_CACHE_ALIGNED     0x0000000000000008ULL
#define POOL_FLAG_RAISE_ON_FAILURE  0x0000000000000020ULL
#define POOL_FLAG_NON_PAGED         0x0000000000000040ULL
#define POOL_FLAG_NON_PAGED_EXECUTE 0x0000000000000080ULL
#define POOL_FLAG_PAGED             0x0000000000000100ULL
#define POOL_FLAG_REQUIRED_END      0x0000000080000000ULL
NTKERNELAPI PVOID NTAPI ExAllocatePool2(_In_ POOL_FLAGS Flags, _In_ SIZE_T NumberOfBytes, _In_ ULONG Tag);
#endif

/* ExInitializeDriverRuntime(DrvRtPoolNxOptIn): makes NonPagedPool mean
   NonPagedPoolNx; a user-space driver host has no executable pool to
   avoid, so it does nothing here. */
#ifndef DrvRtPoolNxOptIn
#define DrvRtPoolNxOptIn 0x00000001
#define ExInitializeDriverRuntime(RuntimeFlags) ((void)(RuntimeFlags))
#endif

/* MmGetSystemAddressForMdlSafe priority flag (Windows 8). */
#ifndef MdlMappingNoExecute
#define MdlMappingNoExecute 0x40000000
#endif

/* KeQuerySystemTimePrecise (Windows 8). */
#ifndef _TUR_KE_QUERY_SYSTEM_TIME_PRECISE
#define _TUR_KE_QUERY_SYSTEM_TIME_PRECISE
NTKERNELAPI VOID NTAPI KeQuerySystemTimePrecise(_Out_ PLARGE_INTEGER CurrentTime);
#endif

/* KeGetCurrentProcessorIndex: the WDK reads it from the processor control
   region (gs:), which in a user-space driver host is the thread's TEB. The
   framework uses it only to index a per-processor table for its bug-check
   callback; the processor number serves. */
#ifndef _TUR_KE_GET_CURRENT_PROCESSOR_INDEX
#define _TUR_KE_GET_CURRENT_PROCESSOR_INDEX
#define KeGetCurrentProcessorIndex() KeGetCurrentProcessorNumberEx(NULL)
#endif

/* Opening a driver's registry keys (Windows 10 2004). */
#ifndef _TUR_DRIVER_REGKEY_TYPE
#define _TUR_DRIVER_REGKEY_TYPE
typedef enum _DRIVER_REGKEY_TYPE {
    DriverRegKeyParameters,
    DriverRegKeyPersistentState,
    DriverRegKeySharedPersistentState
} DRIVER_REGKEY_TYPE, *PDRIVER_REGKEY_TYPE;
NTKERNELAPI NTSTATUS NTAPI IoOpenDriverRegistryKey(_In_ PDRIVER_OBJECT DriverObject,
                                                   _In_ DRIVER_REGKEY_TYPE RegKeyType,
                                                   _In_ ACCESS_MASK DesiredAccess, _In_ ULONG Flags,
                                                   _Out_ PHANDLE DriverRegKey);
#endif

/* Interrupts (Windows 8). */
#ifndef CM_RESOURCE_INTERRUPT_WAKE_HINT
#define CM_RESOURCE_INTERRUPT_WAKE_HINT 0x200
#endif
#ifndef _TUR_IO_REPORT_INTERRUPT_ACTIVE_STATE
#define _TUR_IO_REPORT_INTERRUPT_ACTIVE_STATE
typedef struct _IO_REPORT_INTERRUPT_ACTIVE_STATE_PARAMETERS {
    ULONG Version;
    union {
        PVOID Generic;
        PIO_INTERRUPT_MESSAGE_INFO InterruptMessageTable;
        PKINTERRUPT InterruptObject;
    } ConnectionContext;
} IO_REPORT_INTERRUPT_ACTIVE_STATE_PARAMETERS, *PIO_REPORT_INTERRUPT_ACTIVE_STATE_PARAMETERS;
#endif

/* Pool, registry and integer helpers. */
#ifndef POOL_NX_ALLOCATION
#define POOL_NX_ALLOCATION 512
#endif
#ifndef RTL_QUERY_REGISTRY_TYPECHECK
#define RTL_QUERY_REGISTRY_TYPECHECK 0x00000100
#define RTL_QUERY_REGISTRY_TYPECHECK_SHIFT 24
#endif
#ifndef USHORT_MAX
#define USHORT_MAX 0xffff
#endif
#ifndef _TUR_MORE_KERNEL_FUNCTIONS
#define _TUR_MORE_KERNEL_FUNCTIONS
typedef const WCHAR *LPCWCHAR;
NTKERNELAPI NTSTATUS NTAPI IoGetActivityIdIrp(_In_ PIRP Irp, _Out_ LPGUID Guid);
NTKERNELAPI NTSTATUS NTAPI IoSetActivityIdIrp(_Inout_ PIRP Irp, _In_opt_ LPCGUID Guid);
NTKERNELAPI NTSTATUS NTAPI IoQueryFullDriverPath(_In_ PDRIVER_OBJECT DriverObject, _Out_ PUNICODE_STRING FullPath);
NTKERNELAPI PEPROCESS NTAPI IoGetInitiatorProcess(_In_ PFILE_OBJECT FileObject);
NTKERNELAPI NTSTATUS NTAPI WmiTraceMessageVa(_In_ TRACEHANDLE LoggerHandle, _In_ ULONG MessageFlags,
                                             _In_ LPCGUID MessageGuid, _In_ USHORT MessageNumber,
                                             _In_ va_list MessageArgList);
#endif
#ifndef _TUR_RTL_SIZET_ADD
#define _TUR_RTL_SIZET_ADD
static __inline NTSTATUS RtlSIZETAdd(SIZE_T Augend, SIZE_T Addend, SIZE_T *Result)
{
    if (Augend + Addend < Augend) {
        *Result = (SIZE_T) -1;
        return STATUS_INTEGER_OVERFLOW;
    }
    *Result = Augend + Addend;
    return STATUS_SUCCESS;
}
#endif
#ifndef WDF_VIOLATION
#define WDF_VIOLATION 0x0000010D
#endif
#ifndef PERF_WORKER_THREAD
#define PERF_WORKER_THREAD 0x48000000
#endif

/* The power framework, version 3 (Windows 10). */
#ifndef _TUR_PO_FX_V3
#define _TUR_PO_FX_V3
typedef VOID NTAPI PO_FX_DEVICE_POWER_REQUIRED_CALLBACK(_In_ PVOID Context);
typedef PO_FX_DEVICE_POWER_REQUIRED_CALLBACK *PPO_FX_DEVICE_POWER_REQUIRED_CALLBACK;
typedef VOID NTAPI PO_FX_DEVICE_POWER_NOT_REQUIRED_CALLBACK(_In_ PVOID Context);
typedef PO_FX_DEVICE_POWER_NOT_REQUIRED_CALLBACK *PPO_FX_DEVICE_POWER_NOT_REQUIRED_CALLBACK;
typedef VOID NTAPI PO_FX_DIRECTED_POWER_UP_CALLBACK(_In_ PVOID Context, _In_ ULONG Flags);
typedef PO_FX_DIRECTED_POWER_UP_CALLBACK *PPO_FX_DIRECTED_POWER_UP_CALLBACK;
typedef VOID NTAPI PO_FX_DIRECTED_POWER_DOWN_CALLBACK(_In_ PVOID Context, _In_ ULONG Flags);
typedef PO_FX_DIRECTED_POWER_DOWN_CALLBACK *PPO_FX_DIRECTED_POWER_DOWN_CALLBACK;
#define PO_FX_VERSION_V3 3
#define PO_FX_DEVICE_FLAG_RESERVED_1             0x0000000000000001ULL
#define PO_FX_DEVICE_FLAG_DISABLE_FAST_RESUME    0x0000000000000002ULL
#define PO_FX_DEVICE_FLAG_DFX_CHILDREN_OPTIONAL  0x0000000000000004ULL
typedef struct _PO_FX_DEVICE_V3 {
    ULONG Version;
    ULONGLONG Flags;
    PPO_FX_COMPONENT_ACTIVE_CONDITION_CALLBACK ComponentActiveConditionCallback;
    PPO_FX_COMPONENT_IDLE_CONDITION_CALLBACK ComponentIdleConditionCallback;
    PPO_FX_COMPONENT_IDLE_STATE_CALLBACK ComponentIdleStateCallback;
    PPO_FX_DEVICE_POWER_REQUIRED_CALLBACK DevicePowerRequiredCallback;
    PPO_FX_DEVICE_POWER_NOT_REQUIRED_CALLBACK DevicePowerNotRequiredCallback;
    PPO_FX_POWER_CONTROL_CALLBACK PowerControlCallback;
    PPO_FX_DIRECTED_POWER_UP_CALLBACK DirectedPowerUpCallback;
    PPO_FX_DIRECTED_POWER_DOWN_CALLBACK DirectedPowerDownCallback;
    ULONG DirectedFxTimeoutInSeconds;
    PVOID DeviceContext;
    ULONG ComponentCount;
    PO_FX_COMPONENT_V2 Components[ANYSIZE_ARRAY];
} PO_FX_DEVICE_V3, *PPO_FX_DEVICE_V3;
#define PO_FX_UNKNOWN_POWER 0xFFFFFFFF
#define PO_FX_UNKNOWN_TIME  0xFFFFFFFFFFFFFFFFULL
typedef struct _PO_FX_DEVICE_V1 {
    ULONG Version;
    ULONG ComponentCount;
    PPO_FX_COMPONENT_ACTIVE_CONDITION_CALLBACK ComponentActiveConditionCallback;
    PPO_FX_COMPONENT_IDLE_CONDITION_CALLBACK ComponentIdleConditionCallback;
    PPO_FX_COMPONENT_IDLE_STATE_CALLBACK ComponentIdleStateCallback;
    PPO_FX_DEVICE_POWER_REQUIRED_CALLBACK DevicePowerRequiredCallback;
    PPO_FX_DEVICE_POWER_NOT_REQUIRED_CALLBACK DevicePowerNotRequiredCallback;
    PPO_FX_POWER_CONTROL_CALLBACK PowerControlCallback;
    PVOID DeviceContext;
    PO_FX_COMPONENT_V1 Components[ANYSIZE_ARRAY];
} PO_FX_DEVICE_V1, *PPO_FX_DEVICE_V1;
typedef PO_FX_DEVICE_V1 PO_FX_DEVICE, *PPO_FX_DEVICE;
NTKERNELAPI NTSTATUS NTAPI PoFxRegisterDevice(_In_ PDEVICE_OBJECT Pdo, _In_ PPO_FX_DEVICE Device, _Out_ POHANDLE *Handle);
NTKERNELAPI VOID NTAPI PoFxUnregisterDevice(_In_ POHANDLE Handle);
NTKERNELAPI VOID NTAPI PoFxStartDevicePowerManagement(_In_ POHANDLE Handle);
NTKERNELAPI VOID NTAPI PoFxSetDeviceIdleTimeout(_In_ POHANDLE Handle, _In_ ULONGLONG IdleTimeout);
NTKERNELAPI VOID NTAPI PoFxReportDevicePoweredOn(_In_ POHANDLE Handle);
NTKERNELAPI VOID NTAPI PoFxActivateComponent(_In_ POHANDLE Handle, _In_ ULONG Component, _In_ ULONG Flags);
NTKERNELAPI VOID NTAPI PoFxIdleComponent(_In_ POHANDLE Handle, _In_ ULONG Component, _In_ ULONG Flags);
NTKERNELAPI VOID NTAPI PoFxCompleteIdleState(_In_ POHANDLE Handle, _In_ ULONG Component);
NTKERNELAPI VOID NTAPI PoFxCompleteIdleCondition(_In_ POHANDLE Handle, _In_ ULONG Component);
NTKERNELAPI VOID NTAPI PoFxCompleteDevicePowerNotRequired(_In_ POHANDLE Handle);
NTKERNELAPI VOID NTAPI PoFxCompleteDirectedPowerDown(_In_ POHANDLE Handle);
typedef struct _POWER_PLATFORM_INFORMATION {
    BOOLEAN AoAc;
} POWER_PLATFORM_INFORMATION, *PPOWER_PLATFORM_INFORMATION;
#endif

/* D3cold support interface (Windows 8). */
#ifndef _TUR_D3COLD_SUPPORT_INTERFACE
#define _TUR_D3COLD_SUPPORT_INTERFACE
typedef VOID NTAPI SET_D3COLD_SUPPORT(_In_reads_opt_(_Inexpressible_("varies")) PVOID Context, _In_ BOOLEAN D3ColdSupport);
typedef SET_D3COLD_SUPPORT *PSET_D3COLD_SUPPORT;
typedef enum _DEVICE_WAKE_DEPTH {
    DeviceWakeDepthNotWakeable = 0,
    DeviceWakeDepthD0,
    DeviceWakeDepthD1,
    DeviceWakeDepthD2,
    DeviceWakeDepthD3hot,
    DeviceWakeDepthD3cold,
    DeviceWakeDepthMaximum
} DEVICE_WAKE_DEPTH, *PDEVICE_WAKE_DEPTH;
typedef NTSTATUS NTAPI GET_IDLE_WAKE_INFO(PVOID Context, SYSTEM_POWER_STATE SystemPowerState,
                                          PDEVICE_WAKE_DEPTH DeepestWakeableDstate);
typedef GET_IDLE_WAKE_INFO *PGET_IDLE_WAKE_INFO;
typedef NTSTATUS NTAPI GET_D3COLD_CAPABILITY(PVOID Context, PBOOLEAN D3ColdSupported);
typedef GET_D3COLD_CAPABILITY *PGET_D3COLD_CAPABILITY;
typedef enum _D3COLD_LAST_TRANSITION_STATUS {
    LastDStateTransitionStatusUnknown = 0,
    LastDStateTransitionD3hot,
    LastDStateTransitionD3cold
} D3COLD_LAST_TRANSITION_STATUS, *PD3COLD_LAST_TRANSITION_STATUS;
typedef VOID NTAPI GET_D3COLD_LAST_TRANSITION_STATUS(PVOID Context, PD3COLD_LAST_TRANSITION_STATUS LastTransitionStatus);
typedef GET_D3COLD_LAST_TRANSITION_STATUS *PGET_D3COLD_LAST_TRANSITION_STATUS;
typedef struct _D3COLD_SUPPORT_INTERFACE {
    USHORT Size;
    USHORT Version;
    PVOID Context;
    PINTERFACE_REFERENCE InterfaceReference;
    PINTERFACE_DEREFERENCE InterfaceDereference;
    PSET_D3COLD_SUPPORT SetD3ColdSupport;
    PGET_IDLE_WAKE_INFO GetIdleWakeInfo;
    PGET_D3COLD_CAPABILITY GetD3ColdCapability;
    PGET_D3COLD_CAPABILITY GetBusDriverD3ColdSupport;
    PGET_D3COLD_LAST_TRANSITION_STATUS GetLastTransitionStatus;
} D3COLD_SUPPORT_INTERFACE, *PD3COLD_SUPPORT_INTERFACE;
#define D3COLD_SUPPORT_INTERFACE_VERSION 1

/* Device capabilities with the deepest wakeable D-state per S-state, as
   the framework gathers them (supportkm.cpp). */
typedef struct _STACK_DEVICE_CAPABILITIES {
    DEVICE_CAPABILITIES DeviceCaps;
    DEVICE_WAKE_DEPTH DeepestWakeableDstate[PowerSystemMaximum];
} STACK_DEVICE_CAPABILITIES, *PSTACK_DEVICE_CAPABILITIES;

static __inline DEVICE_POWER_STATE MapWakeDepthToDstate(DEVICE_WAKE_DEPTH WakeDepth)
{
    static const DEVICE_POWER_STATE map[DeviceWakeDepthMaximum] = {
        PowerDeviceD0, PowerDeviceD0, PowerDeviceD1, PowerDeviceD2, PowerDeviceD3, PowerDeviceD3
    };
    if (WakeDepth < 0 || WakeDepth >= DeviceWakeDepthMaximum) {
        return PowerDeviceUnspecified;
    }
    return map[WakeDepth];
}

/* DEVICE_CAPABILITIES in its Windows 8 layout (mingw-w64's ends its bits at
   NoDisplayInUI); used through a cast where the framework sets a newer bit. */
typedef struct _TUR_DEVICE_CAPABILITIES {
    USHORT Size;
    USHORT Version;
    ULONG DeviceD1:1, DeviceD2:1, LockSupported:1, EjectSupported:1, Removable:1, DockDevice:1, UniqueID:1,
          SilentInstall:1, RawDeviceOK:1, SurpriseRemovalOK:1, WakeFromD0:1, WakeFromD1:1, WakeFromD2:1,
          WakeFromD3:1, HardwareDisabled:1, NonDynamic:1, WarmEjectSupported:1, NoDisplayInUI:1, Reserved1:1,
          WakeFromInterrupt:1, SecureDevice:1, ChildOfVgaEnabledBridge:1, DecodeIoOnBoot:1, Reserved:9;
} TUR_DEVICE_CAPABILITIES, *PTUR_DEVICE_CAPABILITIES;

/* IO_RESOURCE_DESCRIPTOR's interrupt member in its Windows 7 layout
   (processor groups); the union keeps its size. */
typedef struct _TUR_IO_RESOURCE_INTERRUPT {
    ULONG MinimumVector;
    ULONG MaximumVector;
    USHORT AffinityPolicy;
    USHORT Group;
    IRQ_PRIORITY PriorityPolicy;
    KAFFINITY TargetedProcessors;
} TUR_IO_RESOURCE_INTERRUPT_LAYOUT, *PTUR_IO_RESOURCE_INTERRUPT_LAYOUT;
#define TUR_IO_RESOURCE_INTERRUPT(Descriptor) ((PTUR_IO_RESOURCE_INTERRUPT_LAYOUT)&(Descriptor)->u.Interrupt)

/* Interfaces and flags. No bus driver in Wine answers these interfaces. */
DEFINE_GUID(GUID_REENUMERATE_SELF_INTERFACE_STANDARD,
    0x2aeb0243, 0x6a6e, 0x486b, 0x82, 0xfc, 0xd8, 0x15, 0xf6, 0xb9, 0x70, 0x06);
DEFINE_GUID(GUID_SECURE_DRIVER_INTERFACE,
    0x7a5e4c01, 0x1d2b, 0x4f6a, 0x9a, 0x01, 0x54, 0x55, 0x52, 0x00, 0x00, 0x10);
#ifndef PNP_DEVICE_ASSIGNED_TO_GUEST
#define PNP_DEVICE_ASSIGNED_TO_GUEST 0x00000100
#endif
DEFINE_GUID(GUID_D3COLD_SUPPORT_INTERFACE, 0xb38290e5, 0x3cd0, 0x4f9d, 0x99, 0x37, 0xf5, 0xfe, 0x2b, 0x44, 0xd4, 0x7a);
#endif

/* DMA version 3 (Windows 8): the types of the members the prelude appends
   to DMA_OPERATIONS and DEVICE_DESCRIPTION. */
#ifndef _TUR_DMA_V3
#define _TUR_DMA_V3
typedef VOID NTAPI DMA_COMPLETION_ROUTINE(_In_ PDMA_ADAPTER DmaAdapter, _In_ PDEVICE_OBJECT DeviceObject,
                                          _In_ PVOID CompletionContext, _In_ DMA_COMPLETION_STATUS Status);
typedef DMA_COMPLETION_ROUTINE *PDMA_COMPLETION_ROUTINE;
#define DEVICE_DESCRIPTION_VERSION3 3
#define DMA_TRANSFER_INFO_VERSION1 1
#define DMA_TRANSFER_CONTEXT_VERSION1 1
#define DMA_TRANSFER_CONTEXT_SIZE_V1 64
#define DMA_SYNCHRONOUS_CALLBACK 0x01
#define DMA_ZERO_BUFFERS 0x02
typedef struct _DMA_TRANSFER_INFO_V1 {
    ULONG MapRegisterCount;
    ULONG ScatterGatherElementCount;
    ULONG ScatterGatherListSize;
} DMA_TRANSFER_INFO_V1, *PDMA_TRANSFER_INFO_V1;
typedef struct _DMA_TRANSFER_INFO {
    ULONG Version;
    union {
        DMA_TRANSFER_INFO_V1 V1;
    };
} DMA_TRANSFER_INFO, *PDMA_TRANSFER_INFO;
#endif

/* ntstrsafe.h's UNICODE_STRING functions, as documented. */
#ifndef _TUR_RTL_UNICODE_STRING_SAFE
#define _TUR_RTL_UNICODE_STRING_SAFE
static __inline NTSTATUS RtlUnicodeStringPrintf(PUNICODE_STRING DestinationString, NTSTRSAFE_PCWSTR Format, ...)
{
    NTSTATUS status;
    va_list args;
    size_t length = 0;

    if (DestinationString == NULL || DestinationString->Buffer == NULL || DestinationString->MaximumLength == 0) {
        return STATUS_INVALID_PARAMETER;
    }
    va_start(args, Format);
    status = RtlStringCbVPrintfW(DestinationString->Buffer, DestinationString->MaximumLength, Format, args);
    va_end(args);
    if (NT_SUCCESS(status) || status == STATUS_BUFFER_OVERFLOW) {
        while (length * sizeof(WCHAR) < DestinationString->MaximumLength && DestinationString->Buffer[length]) {
            length++;
        }
        DestinationString->Length = (USHORT)(length * sizeof(WCHAR));
    }
    return status;
}

static __inline NTSTATUS RtlUnicodeStringCopy(PUNICODE_STRING DestinationString, PCUNICODE_STRING SourceString)
{
    if (DestinationString == NULL || SourceString == NULL) {
        return STATUS_INVALID_PARAMETER;
    }
    if (SourceString->Length > DestinationString->MaximumLength) {
        RtlCopyMemory(DestinationString->Buffer, SourceString->Buffer, DestinationString->MaximumLength);
        DestinationString->Length = DestinationString->MaximumLength;
        return STATUS_BUFFER_OVERFLOW;
    }
    RtlCopyMemory(DestinationString->Buffer, SourceString->Buffer, SourceString->Length);
    DestinationString->Length = SourceString->Length;
    return STATUS_SUCCESS;
}
#endif

#ifdef __cplusplus
}

/* The framework passes MmGetSystemAddressForMdlSafe a priority ORed with
   MdlMappingNoExecute, an unsigned long; MSVC converts it to the
   MM_PAGE_PRIORITY parameter of mingw-w64's prototype silently, a standard
   C++ compiler does not. This overload takes the ULONG and converts. */
static __inline PVOID MmMapLockedPagesSpecifyCache(PMDL MemoryDescriptorList, KPROCESSOR_MODE AccessMode,
                                                   MEMORY_CACHING_TYPE CacheType, PVOID BaseAddress,
                                                   ULONG BugCheckOnFailure, ULONG Priority)
{
    return MmMapLockedPagesSpecifyCache(MemoryDescriptorList, AccessMode, CacheType, BaseAddress,
                                        BugCheckOnFailure, (MM_PAGE_PRIORITY) Priority);
}
#endif
