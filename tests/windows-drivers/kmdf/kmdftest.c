// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// kmdftest.sys: a minimal KMDF USB function driver written for the
// experiment, for the TUR loopback test device (1209:0001: bulk OUT 0x01
// echoed on bulk IN 0x81, vendor requests 0x5b/0x5c). It exercises, one step
// at a time, what every KMDF USB driver does -- WdfDriverCreate, an
// EvtDriverDeviceAdd that creates a WDFDEVICE with a default queue and a
// device interface, EvtDevicePrepareHardware that creates the USB target,
// selects the configuration and finds the pipes, EvtDeviceD0Entry -- and
// offers IOCTLs for bulk and vendor transfers through the framework's
// synchronous USB DDIs and a self-test of WDFDPC, WDFTIMER and WDFWORKITEM.
// Every step logs with DbgPrint ("kmdftest: ..."), which Wine shows on its
// debugstr channel.
//
// Built against Microsoft's public KMDF 1.33 headers with fxstub.c; loaded
// only by Wine's winedevice.exe, a Linux user-space process.
#include <ntddk.h>
#include <initguid.h>
#include <wdf.h>
#include <usb.h>
#include <usbdlib.h>
#include <wdfusb.h>

#include "kmdftest.h"

DEFINE_GUID(GUID_DEVINTERFACE_KMDFTEST, 0x6c1f3e6a, 0x8d1b, 0x4b1e, 0x9f, 0x42, 0x54, 0x55, 0x52, 0x44, 0x46, 0x0a);

typedef struct _DEVICE_CONTEXT {
    WDFUSBDEVICE UsbDevice;
    WDFUSBINTERFACE Interface;
    WDFUSBPIPE BulkIn;
    WDFUSBPIPE BulkOut;
    struct kmdftest_info Info;
    KEVENT DpcEvent;
    KEVENT TimerEvent;
    KEVENT WorkItemEvent;
    LONG DpcRuns;
    LONG TimerRuns;
    LONG WorkItemRuns;
} DEVICE_CONTEXT, *PDEVICE_CONTEXT;
WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(DEVICE_CONTEXT, GetDeviceContext)

typedef struct _CALLBACK_CONTEXT {
    PDEVICE_CONTEXT Device;
} CALLBACK_CONTEXT, *PCALLBACK_CONTEXT;
WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(CALLBACK_CONTEXT, GetCallbackContext)

DRIVER_INITIALIZE DriverEntry;
static EVT_WDF_DRIVER_DEVICE_ADD EvtDeviceAdd;
static EVT_WDF_DEVICE_PREPARE_HARDWARE EvtPrepareHardware;
static EVT_WDF_DEVICE_D0_ENTRY EvtD0Entry;
static EVT_WDF_DEVICE_D0_EXIT EvtD0Exit;
static EVT_WDF_IO_QUEUE_IO_DEVICE_CONTROL EvtIoDeviceControl;
static EVT_WDF_DPC EvtDpc;
static EVT_WDF_TIMER EvtTimer;
static EVT_WDF_WORKITEM EvtWorkItem;

NTSTATUS DriverEntry(PDRIVER_OBJECT DriverObject, PUNICODE_STRING RegistryPath)
{
    WDF_DRIVER_CONFIG config;
    NTSTATUS status;

    DbgPrint("kmdftest: DriverEntry\n");
    WDF_DRIVER_CONFIG_INIT(&config, EvtDeviceAdd);
    status = WdfDriverCreate(DriverObject, RegistryPath, WDF_NO_OBJECT_ATTRIBUTES, &config, WDF_NO_HANDLE);
    DbgPrint("kmdftest: WdfDriverCreate returned 0x%08lx\n", (ULONG) status);
    return status;
}

static NTSTATUS EvtDeviceAdd(WDFDRIVER Driver, PWDFDEVICE_INIT DeviceInit)
{
    WDF_PNPPOWER_EVENT_CALLBACKS pnp;
    WDF_OBJECT_ATTRIBUTES attributes;
    WDF_IO_QUEUE_CONFIG queue;
    PDEVICE_CONTEXT ctx;
    WDFDEVICE device;
    NTSTATUS status;

    UNREFERENCED_PARAMETER(Driver);
    DbgPrint("kmdftest: EvtDriverDeviceAdd\n");

    WDF_PNPPOWER_EVENT_CALLBACKS_INIT(&pnp);
    pnp.EvtDevicePrepareHardware = EvtPrepareHardware;
    pnp.EvtDeviceD0Entry = EvtD0Entry;
    pnp.EvtDeviceD0Exit = EvtD0Exit;
    WdfDeviceInitSetPnpPowerEventCallbacks(DeviceInit, &pnp);
    WdfDeviceInitSetIoType(DeviceInit, WdfDeviceIoBuffered);

    WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attributes, DEVICE_CONTEXT);
    status = WdfDeviceCreate(&DeviceInit, &attributes, &device);
    DbgPrint("kmdftest: WdfDeviceCreate returned 0x%08lx\n", (ULONG) status);
    if (!NT_SUCCESS(status)) {
        return status;
    }
    ctx = GetDeviceContext(device);
    RtlCopyMemory(ctx->Info.tag, "TUR-KMDFTEST", sizeof("TUR-KMDFTEST"));
    ctx->Info.prepare_status = (long) STATUS_PENDING;
    KeInitializeEvent(&ctx->DpcEvent, NotificationEvent, FALSE);
    KeInitializeEvent(&ctx->TimerEvent, NotificationEvent, FALSE);
    KeInitializeEvent(&ctx->WorkItemEvent, NotificationEvent, FALSE);

    WDF_IO_QUEUE_CONFIG_INIT_DEFAULT_QUEUE(&queue, WdfIoQueueDispatchSequential);
    queue.EvtIoDeviceControl = EvtIoDeviceControl;
    status = WdfIoQueueCreate(device, &queue, WDF_NO_OBJECT_ATTRIBUTES, WDF_NO_HANDLE);
    DbgPrint("kmdftest: WdfIoQueueCreate returned 0x%08lx\n", (ULONG) status);
    if (!NT_SUCCESS(status)) {
        return status;
    }
    status = WdfDeviceCreateDeviceInterface(device, &GUID_DEVINTERFACE_KMDFTEST, NULL);
    DbgPrint("kmdftest: WdfDeviceCreateDeviceInterface returned 0x%08lx\n", (ULONG) status);
    return status;
}

static NTSTATUS EvtPrepareHardware(WDFDEVICE Device, WDFCMRESLIST ResourcesRaw, WDFCMRESLIST ResourcesTranslated)
{
    PDEVICE_CONTEXT ctx = GetDeviceContext(Device);
    WDF_USB_DEVICE_CREATE_CONFIG create;
    WDF_USB_DEVICE_INFORMATION information;
    WDF_USB_DEVICE_SELECT_CONFIG_PARAMS select;
    USB_DEVICE_DESCRIPTOR descriptor;
    NTSTATUS status;
    UCHAR i, pipes;

    UNREFERENCED_PARAMETER(ResourcesRaw);
    UNREFERENCED_PARAMETER(ResourcesTranslated);
    ctx->Info.events |= KMDFTEST_EVENT_PREPARE_HARDWARE;
    DbgPrint("kmdftest: EvtDevicePrepareHardware\n");

    if (ctx->UsbDevice == NULL) {
        WDF_USB_DEVICE_CREATE_CONFIG_INIT(&create, USBD_CLIENT_CONTRACT_VERSION_602);
        status = WdfUsbTargetDeviceCreateWithParameters(Device, &create, WDF_NO_OBJECT_ATTRIBUTES, &ctx->UsbDevice);
        DbgPrint("kmdftest: WdfUsbTargetDeviceCreateWithParameters returned 0x%08lx\n", (ULONG) status);
        if (!NT_SUCCESS(status)) {
            status = WdfUsbTargetDeviceCreate(Device, WDF_NO_OBJECT_ATTRIBUTES, &ctx->UsbDevice);
            DbgPrint("kmdftest: WdfUsbTargetDeviceCreate returned 0x%08lx\n", (ULONG) status);
        }
        if (!NT_SUCCESS(status)) {
            ctx->Info.prepare_status = status;
            return STATUS_SUCCESS;  // keep the device so the program can read the result
        }
    }

    WdfUsbTargetDeviceGetDeviceDescriptor(ctx->UsbDevice, &descriptor);
    ctx->Info.vendor = descriptor.idVendor;
    ctx->Info.product = descriptor.idProduct;
    DbgPrint("kmdftest: device descriptor %04x:%04x bcdUSB %04x\n", descriptor.idVendor, descriptor.idProduct,
             descriptor.bcdUSB);

    WDF_USB_DEVICE_INFORMATION_INIT(&information);
    status = WdfUsbTargetDeviceRetrieveInformation(ctx->UsbDevice, &information);
    DbgPrint("kmdftest: WdfUsbTargetDeviceRetrieveInformation returned 0x%08lx traits 0x%lx usbdi 0x%lx\n",
             (ULONG) status, information.Traits, information.UsbdVersionInformation.USBDI_Version);
    if (NT_SUCCESS(status)) {
        ctx->Info.traits = information.Traits;
        ctx->Info.usbd_version = information.UsbdVersionInformation.USBDI_Version;
    }

    WDF_USB_DEVICE_SELECT_CONFIG_PARAMS_INIT_SINGLE_INTERFACE(&select);
    status = WdfUsbTargetDeviceSelectConfig(ctx->UsbDevice, WDF_NO_OBJECT_ATTRIBUTES, &select);
    DbgPrint("kmdftest: WdfUsbTargetDeviceSelectConfig returned 0x%08lx\n", (ULONG) status);
    if (!NT_SUCCESS(status)) {
        ctx->Info.prepare_status = status;
        return STATUS_SUCCESS;
    }
    ctx->Info.events |= KMDFTEST_EVENT_SELECT_CONFIG;
    ctx->Interface = select.Types.SingleInterface.ConfiguredUsbInterface;
    ctx->Info.num_interfaces = WdfUsbTargetDeviceGetNumInterfaces(ctx->UsbDevice);
    pipes = select.Types.SingleInterface.NumberConfiguredPipes;
    ctx->Info.num_pipes = pipes;
    for (i = 0; i < pipes; i++) {
        WDF_USB_PIPE_INFORMATION pipeInfo;
        WDFUSBPIPE pipe;

        WDF_USB_PIPE_INFORMATION_INIT(&pipeInfo);
        pipe = WdfUsbInterfaceGetConfiguredPipe(ctx->Interface, i, &pipeInfo);
        DbgPrint("kmdftest: pipe %u endpoint 0x%02x type %d max packet %lu\n", i, pipeInfo.EndpointAddress,
                 (int) pipeInfo.PipeType, pipeInfo.MaximumPacketSize);
        WdfUsbTargetPipeSetNoMaximumPacketSizeCheck(pipe);
        if (pipeInfo.PipeType == WdfUsbPipeTypeBulk && WdfUsbTargetPipeIsInEndpoint(pipe)) {
            ctx->BulkIn = pipe;
            ctx->Info.bulk_in = pipeInfo.EndpointAddress;
        }
        if (pipeInfo.PipeType == WdfUsbPipeTypeBulk && WdfUsbTargetPipeIsOutEndpoint(pipe)) {
            ctx->BulkOut = pipe;
            ctx->Info.bulk_out = pipeInfo.EndpointAddress;
        }
    }
    if (ctx->BulkIn != NULL && ctx->BulkOut != NULL) {
        ctx->Info.events |= KMDFTEST_EVENT_PIPES;
    }
    ctx->Info.prepare_status = STATUS_SUCCESS;
    return STATUS_SUCCESS;
}

static NTSTATUS EvtD0Entry(WDFDEVICE Device, WDF_POWER_DEVICE_STATE PreviousState)
{
    PDEVICE_CONTEXT ctx = GetDeviceContext(Device);

    ctx->Info.events |= KMDFTEST_EVENT_D0_ENTRY;
    ctx->Info.d0_entries++;
    DbgPrint("kmdftest: EvtDeviceD0Entry from D%d\n", (int) PreviousState - 1);
    return STATUS_SUCCESS;
}

static NTSTATUS EvtD0Exit(WDFDEVICE Device, WDF_POWER_DEVICE_STATE TargetState)
{
    UNREFERENCED_PARAMETER(Device);
    DbgPrint("kmdftest: EvtDeviceD0Exit to D%d\n", (int) TargetState - 1);
    return STATUS_SUCCESS;
}

static VOID EvtDpc(WDFDPC Dpc)
{
    PDEVICE_CONTEXT ctx = GetCallbackContext(Dpc)->Device;

    InterlockedIncrement(&ctx->DpcRuns);
    KeSetEvent(&ctx->DpcEvent, IO_NO_INCREMENT, FALSE);
}

static VOID EvtTimer(WDFTIMER Timer)
{
    PDEVICE_CONTEXT ctx = GetCallbackContext(Timer)->Device;

    InterlockedIncrement(&ctx->TimerRuns);
    KeSetEvent(&ctx->TimerEvent, IO_NO_INCREMENT, FALSE);
}

static VOID EvtWorkItem(WDFWORKITEM WorkItem)
{
    PDEVICE_CONTEXT ctx = GetCallbackContext(WorkItem)->Device;

    InterlockedIncrement(&ctx->WorkItemRuns);
    KeSetEvent(&ctx->WorkItemEvent, IO_NO_INCREMENT, FALSE);
}

static ULONG ElapsedMs(ULONGLONG Start)
{
    return (ULONG) ((KeQueryInterruptTime() - Start) / 10000);
}

static NTSTATUS SelfTest(WDFDEVICE Device, struct kmdftest_selftest *Result)
{
    PDEVICE_CONTEXT ctx = GetDeviceContext(Device);
    WDF_OBJECT_ATTRIBUTES attributes;
    LARGE_INTEGER timeout;
    WDF_DPC_CONFIG dpcConfig;
    WDF_TIMER_CONFIG timerConfig;
    WDF_WORKITEM_CONFIG workConfig;
    WDFDPC dpc;
    WDFTIMER timer;
    WDFWORKITEM workItem;
    ULONGLONG start;
    NTSTATUS status;

    RtlZeroMemory(Result, sizeof(*Result));
    timeout.QuadPart = -20000000LL;  // 2 s

    WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attributes, CALLBACK_CONTEXT);
    attributes.ParentObject = Device;
    WDF_DPC_CONFIG_INIT(&dpcConfig, EvtDpc);
    dpcConfig.AutomaticSerialization = FALSE;
    status = WdfDpcCreate(&dpcConfig, &attributes, &dpc);
    DbgPrint("kmdftest: WdfDpcCreate returned 0x%08lx\n", (ULONG) status);
    if (NT_SUCCESS(status)) {
        GetCallbackContext(dpc)->Device = ctx;
        KeClearEvent(&ctx->DpcEvent);
        start = KeQueryInterruptTime();
        WdfDpcEnqueue(dpc);
        Result->dpc_status = KeWaitForSingleObject(&ctx->DpcEvent, Executive, KernelMode, FALSE, &timeout);
        Result->dpc_ms = ElapsedMs(start);
        Result->dpc_runs = ctx->DpcRuns;
        DbgPrint("kmdftest: WDFDPC wait 0x%08lx runs %ld\n", (ULONG) Result->dpc_status, ctx->DpcRuns);
        WdfObjectDelete(dpc);
    }
    else {
        Result->dpc_status = status;
    }

    WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attributes, CALLBACK_CONTEXT);
    attributes.ParentObject = Device;
    WDF_TIMER_CONFIG_INIT(&timerConfig, EvtTimer);
    timerConfig.AutomaticSerialization = FALSE;
    status = WdfTimerCreate(&timerConfig, &attributes, &timer);
    DbgPrint("kmdftest: WdfTimerCreate returned 0x%08lx\n", (ULONG) status);
    if (NT_SUCCESS(status)) {
        GetCallbackContext(timer)->Device = ctx;
        KeClearEvent(&ctx->TimerEvent);
        start = KeQueryInterruptTime();
        WdfTimerStart(timer, WDF_REL_TIMEOUT_IN_MS(100));
        Result->timer_status = KeWaitForSingleObject(&ctx->TimerEvent, Executive, KernelMode, FALSE, &timeout);
        Result->timer_ms = ElapsedMs(start);
        Result->timer_runs = ctx->TimerRuns;
        DbgPrint("kmdftest: WDFTIMER wait 0x%08lx runs %ld\n", (ULONG) Result->timer_status, ctx->TimerRuns);
        WdfTimerStop(timer, TRUE);
        WdfObjectDelete(timer);
    }
    else {
        Result->timer_status = status;
    }

    WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attributes, CALLBACK_CONTEXT);
    attributes.ParentObject = Device;
    WDF_WORKITEM_CONFIG_INIT(&workConfig, EvtWorkItem);
    workConfig.AutomaticSerialization = FALSE;
    status = WdfWorkItemCreate(&workConfig, &attributes, &workItem);
    DbgPrint("kmdftest: WdfWorkItemCreate returned 0x%08lx\n", (ULONG) status);
    if (NT_SUCCESS(status)) {
        GetCallbackContext(workItem)->Device = ctx;
        KeClearEvent(&ctx->WorkItemEvent);
        start = KeQueryInterruptTime();
        WdfWorkItemEnqueue(workItem);
        Result->workitem_status = KeWaitForSingleObject(&ctx->WorkItemEvent, Executive, KernelMode, FALSE, &timeout);
        Result->workitem_ms = ElapsedMs(start);
        Result->workitem_runs = ctx->WorkItemRuns;
        DbgPrint("kmdftest: WDFWORKITEM wait 0x%08lx runs %ld\n", (ULONG) Result->workitem_status,
                 ctx->WorkItemRuns);
        WdfWorkItemFlush(workItem);
        WdfObjectDelete(workItem);
    }
    else {
        Result->workitem_status = status;
    }
    return STATUS_SUCCESS;
}

static VOID EvtIoDeviceControl(WDFQUEUE Queue, WDFREQUEST Request, size_t OutputBufferLength,
                               size_t InputBufferLength, ULONG IoControlCode)
{
    WDFDEVICE device = WdfIoQueueGetDevice(Queue);
    PDEVICE_CONTEXT ctx = GetDeviceContext(device);
    WDF_REQUEST_SEND_OPTIONS options;
    WDF_MEMORY_DESCRIPTOR memory;
    WDF_USB_CONTROL_SETUP_PACKET setup;
    PVOID in = NULL, out = NULL;
    size_t inLength = 0, outLength = 0;
    ULONG transferred = 0;
    ULONG_PTR information = 0;
    NTSTATUS status;

    WDF_REQUEST_SEND_OPTIONS_INIT(&options, 0);
    WDF_REQUEST_SEND_OPTIONS_SET_TIMEOUT(&options, WDF_REL_TIMEOUT_IN_SEC(5));

    switch (IoControlCode) {
    case KMDFTEST_IOCTL_INFO:
        status = WdfRequestRetrieveOutputBuffer(Request, sizeof(ctx->Info), &out, NULL);
        if (NT_SUCCESS(status)) {
            RtlCopyMemory(out, &ctx->Info, sizeof(ctx->Info));
            information = sizeof(ctx->Info);
        }
        break;

    case KMDFTEST_IOCTL_ECHO:
    case KMDFTEST_IOCTL_VENDOR:
        if (ctx->BulkIn == NULL || ctx->BulkOut == NULL || InputBufferLength == 0 || OutputBufferLength == 0) {
            status = STATUS_INVALID_DEVICE_STATE;
            break;
        }
        status = WdfRequestRetrieveInputBuffer(Request, 1, &in, &inLength);
        if (NT_SUCCESS(status)) {
            status = WdfRequestRetrieveOutputBuffer(Request, 1, &out, &outLength);
        }
        if (!NT_SUCCESS(status)) {
            break;
        }
        WDF_MEMORY_DESCRIPTOR_INIT_BUFFER(&memory, in, (ULONG) inLength);
        if (IoControlCode == KMDFTEST_IOCTL_ECHO) {
            status = WdfUsbTargetPipeWriteSynchronously(ctx->BulkOut, NULL, &options, &memory, &transferred);
        }
        else {
            WDF_USB_CONTROL_SETUP_PACKET_INIT_VENDOR(&setup, BmRequestHostToDevice, BmRequestToDevice, 0x5b, 0, 0);
            status = WdfUsbTargetDeviceSendControlTransferSynchronously(ctx->UsbDevice, NULL, &options, &setup,
                                                                        &memory, &transferred);
        }
        DbgPrint("kmdftest: OUT transfer of %lu bytes: 0x%08lx, %lu done\n", (ULONG) inLength, (ULONG) status,
                 transferred);
        if (!NT_SUCCESS(status)) {
            break;
        }
        WDF_MEMORY_DESCRIPTOR_INIT_BUFFER(&memory, out, (ULONG) outLength);
        transferred = 0;
        if (IoControlCode == KMDFTEST_IOCTL_ECHO) {
            status = WdfUsbTargetPipeReadSynchronously(ctx->BulkIn, NULL, &options, &memory, &transferred);
        }
        else {
            WDF_USB_CONTROL_SETUP_PACKET_INIT_VENDOR(&setup, BmRequestDeviceToHost, BmRequestToDevice, 0x5c, 0, 0);
            status = WdfUsbTargetDeviceSendControlTransferSynchronously(ctx->UsbDevice, NULL, &options, &setup,
                                                                        &memory, &transferred);
        }
        DbgPrint("kmdftest: IN transfer of up to %lu bytes: 0x%08lx, %lu done\n", (ULONG) outLength,
                 (ULONG) status, transferred);
        information = transferred;
        break;

    case KMDFTEST_IOCTL_SELFTEST:
        status = WdfRequestRetrieveOutputBuffer(Request, sizeof(struct kmdftest_selftest), &out, NULL);
        if (NT_SUCCESS(status)) {
            status = SelfTest(device, (struct kmdftest_selftest *) out);
            information = sizeof(struct kmdftest_selftest);
        }
        break;

    default:
        status = STATUS_INVALID_DEVICE_REQUEST;
        break;
    }
    WdfRequestCompleteWithInformation(Request, status, information);
}
