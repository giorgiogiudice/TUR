// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// SEALED (driver-synthesis experiment): qfdesk.sys, the Windows KMDF driver
// of the invented desk device (1209:000c; protocol in ../PROTOCOL.md).
// Built like the windows-drivers experiment's kmdftest.sys: against
// Microsoft's public KMDF 1.33 headers with TUR's stub (fxstub.c), loaded
// only by Wine's winedevice.exe, a Linux user-space process, on the KMDF
// runtime built from Microsoft's MIT source.
//
// What it does:
//   - EvtDevicePrepareHardware: the USB target, the configuration, the bulk
//     OUT/IN and interrupt IN pipes, a continuous reader on the interrupt
//     pipe that keeps the device's event reports in a small ring;
//   - IOCTLs: CONNECT (the handshake), DISCONNECT, TRANSACT (one command
//     frame out, its response frame(s) in, checked), GET_EVENT (pops one
//     report, never waits), STATS.
// Every request is completed synchronously; nothing waits on a transfer
// the device will not complete (Wine's wineusb.sys cannot cancel one).
#include <ntddk.h>
#include <initguid.h>
#include <wdf.h>
#include <usb.h>
#include <usbdlib.h>
#include <wdfusb.h>

#include "qfdesk.h"

DEFINE_GUID(GUID_DEVINTERFACE_QFDESK, 0x3a8e5c41, 0x7b2d, 0x4f19, 0xa0, 0xc6, 0x51, 0x46, 0x44, 0x31, 0xc0, 0xde);

#ifndef STATUS_DEVICE_PROTOCOL_ERROR
#define STATUS_DEVICE_PROTOCOL_ERROR ((NTSTATUS) 0xC0000186L)
#endif
#ifndef STATUS_CRC_ERROR
#define STATUS_CRC_ERROR ((NTSTATUS) 0xC000003FL)
#endif
#ifndef STATUS_DEVICE_CONFIGURATION_ERROR
#define STATUS_DEVICE_CONFIGURATION_ERROR ((NTSTATUS) 0xC0000182L)
#endif
#ifndef STATUS_DEVICE_NOT_CONNECTED
#define STATUS_DEVICE_NOT_CONNECTED ((NTSTATUS) 0xC000009DL)
#endif

#define EVENT_RING 64
#define FRAME_BUFFER 512

typedef struct _DEVICE_CONTEXT {
    WDFUSBDEVICE UsbDevice;
    WDFUSBINTERFACE Interface;
    WDFUSBPIPE BulkIn;
    WDFUSBPIPE BulkOut;
    WDFUSBPIPE InterruptIn;
    BOOLEAN ReaderConfigured;
    WDFSPINLOCK EventLock;
    UCHAR Events[EVENT_RING][8];
    ULONG EventHead;
    ULONG EventCount;
    UCHAR Seq;
    BOOLEAN Connected;
    UCHAR Out[FRAME_BUFFER];
    UCHAR In[FRAME_BUFFER];
    struct qfdesk_command Command;
    struct qfdesk_stats Stats;
} DEVICE_CONTEXT, *PDEVICE_CONTEXT;
WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(DEVICE_CONTEXT, GetDeviceContext)

DRIVER_INITIALIZE DriverEntry;
static EVT_WDF_DRIVER_DEVICE_ADD EvtDeviceAdd;
static EVT_WDF_DEVICE_PREPARE_HARDWARE EvtPrepareHardware;
static EVT_WDF_DEVICE_D0_ENTRY EvtD0Entry;
static EVT_WDF_DEVICE_D0_EXIT EvtD0Exit;
static EVT_WDF_IO_QUEUE_IO_DEVICE_CONTROL EvtIoDeviceControl;
static EVT_WDF_USB_READER_COMPLETION_ROUTINE EvtEventReadComplete;
static EVT_WDF_USB_READERS_FAILED EvtEventReadersFailed;

NTSTATUS DriverEntry(PDRIVER_OBJECT DriverObject, PUNICODE_STRING RegistryPath)
{
    WDF_DRIVER_CONFIG config;
    NTSTATUS status;

    DbgPrint("qfdesk: DriverEntry\n");
    WDF_DRIVER_CONFIG_INIT(&config, EvtDeviceAdd);
    status = WdfDriverCreate(DriverObject, RegistryPath, WDF_NO_OBJECT_ATTRIBUTES, &config, WDF_NO_HANDLE);
    DbgPrint("qfdesk: WdfDriverCreate returned 0x%08lx\n", (ULONG) status);
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
    DbgPrint("qfdesk: EvtDriverDeviceAdd\n");

    WDF_PNPPOWER_EVENT_CALLBACKS_INIT(&pnp);
    pnp.EvtDevicePrepareHardware = EvtPrepareHardware;
    pnp.EvtDeviceD0Entry = EvtD0Entry;
    pnp.EvtDeviceD0Exit = EvtD0Exit;
    WdfDeviceInitSetPnpPowerEventCallbacks(DeviceInit, &pnp);
    WdfDeviceInitSetIoType(DeviceInit, WdfDeviceIoBuffered);

    WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attributes, DEVICE_CONTEXT);
    status = WdfDeviceCreate(&DeviceInit, &attributes, &device);
    DbgPrint("qfdesk: WdfDeviceCreate returned 0x%08lx\n", (ULONG) status);
    if (!NT_SUCCESS(status)) {
        return status;
    }
    ctx = GetDeviceContext(device);
    RtlCopyMemory(ctx->Stats.tag, "QFDESK", sizeof("QFDESK"));
    ctx->Stats.prepare_status = (ULONG) STATUS_PENDING;

    WDF_OBJECT_ATTRIBUTES_INIT(&attributes);
    attributes.ParentObject = device;
    status = WdfSpinLockCreate(&attributes, &ctx->EventLock);
    DbgPrint("qfdesk: WdfSpinLockCreate returned 0x%08lx\n", (ULONG) status);
    if (!NT_SUCCESS(status)) {
        return status;
    }

    WDF_IO_QUEUE_CONFIG_INIT_DEFAULT_QUEUE(&queue, WdfIoQueueDispatchSequential);
    queue.EvtIoDeviceControl = EvtIoDeviceControl;
    status = WdfIoQueueCreate(device, &queue, WDF_NO_OBJECT_ATTRIBUTES, WDF_NO_HANDLE);
    DbgPrint("qfdesk: WdfIoQueueCreate returned 0x%08lx\n", (ULONG) status);
    if (!NT_SUCCESS(status)) {
        return status;
    }
    status = WdfDeviceCreateDeviceInterface(device, &GUID_DEVINTERFACE_QFDESK, NULL);
    DbgPrint("qfdesk: WdfDeviceCreateDeviceInterface returned 0x%08lx\n", (ULONG) status);
    return status;
}

static VOID EvtEventReadComplete(WDFUSBPIPE Pipe, WDFMEMORY Buffer, size_t NumBytesTransferred, WDFCONTEXT Context)
{
    PDEVICE_CONTEXT ctx = (PDEVICE_CONTEXT) Context;
    PUCHAR data;
    ULONG slot;

    UNREFERENCED_PARAMETER(Pipe);
    if (NumBytesTransferred == 0) {
        return;
    }
    data = (PUCHAR) WdfMemoryGetBuffer(Buffer, NULL);
    WdfSpinLockAcquire(ctx->EventLock);
    ctx->Stats.events_received++;
    if (ctx->EventCount == EVENT_RING) {
        ctx->EventHead = (ctx->EventHead + 1) % EVENT_RING;  // drop the oldest
        ctx->EventCount--;
        ctx->Stats.events_dropped++;
    }
    slot = (ctx->EventHead + ctx->EventCount) % EVENT_RING;
    RtlZeroMemory(ctx->Events[slot], 8);
    RtlCopyMemory(ctx->Events[slot], data, NumBytesTransferred < 8 ? NumBytesTransferred : 8);
    ctx->EventCount++;
    WdfSpinLockRelease(ctx->EventLock);
}

static BOOLEAN EvtEventReadersFailed(WDFUSBPIPE Pipe, NTSTATUS Status, USBD_STATUS UsbdStatus)
{
    UNREFERENCED_PARAMETER(Pipe);
    DbgPrint("qfdesk: event reader failed 0x%08lx usbd 0x%08lx\n", (ULONG) Status, (ULONG) UsbdStatus);
    return TRUE;  // let the framework restart the readers
}

static NTSTATUS EvtPrepareHardware(WDFDEVICE Device, WDFCMRESLIST ResourcesRaw, WDFCMRESLIST ResourcesTranslated)
{
    PDEVICE_CONTEXT ctx = GetDeviceContext(Device);
    WDF_USB_DEVICE_CREATE_CONFIG create;
    WDF_USB_DEVICE_SELECT_CONFIG_PARAMS select;
    WDF_USB_CONTINUOUS_READER_CONFIG reader;
    NTSTATUS status;
    UCHAR i, pipes;

    UNREFERENCED_PARAMETER(ResourcesRaw);
    UNREFERENCED_PARAMETER(ResourcesTranslated);
    DbgPrint("qfdesk: EvtDevicePrepareHardware\n");

    if (ctx->UsbDevice == NULL) {
        WDF_USB_DEVICE_CREATE_CONFIG_INIT(&create, USBD_CLIENT_CONTRACT_VERSION_602);
        status = WdfUsbTargetDeviceCreateWithParameters(Device, &create, WDF_NO_OBJECT_ATTRIBUTES, &ctx->UsbDevice);
        if (!NT_SUCCESS(status)) {
            status = WdfUsbTargetDeviceCreate(Device, WDF_NO_OBJECT_ATTRIBUTES, &ctx->UsbDevice);
        }
        DbgPrint("qfdesk: USB target 0x%08lx\n", (ULONG) status);
        if (!NT_SUCCESS(status)) {
            ctx->Stats.prepare_status = (ULONG) status;
            return STATUS_SUCCESS;  // keep the device so that STATS can report it
        }
    }

    WDF_USB_DEVICE_SELECT_CONFIG_PARAMS_INIT_SINGLE_INTERFACE(&select);
    status = WdfUsbTargetDeviceSelectConfig(ctx->UsbDevice, WDF_NO_OBJECT_ATTRIBUTES, &select);
    DbgPrint("qfdesk: WdfUsbTargetDeviceSelectConfig returned 0x%08lx\n", (ULONG) status);
    if (!NT_SUCCESS(status)) {
        ctx->Stats.prepare_status = (ULONG) status;
        return STATUS_SUCCESS;
    }
    ctx->Interface = select.Types.SingleInterface.ConfiguredUsbInterface;
    pipes = select.Types.SingleInterface.NumberConfiguredPipes;
    ctx->Stats.pipes = pipes;
    for (i = 0; i < pipes; i++) {
        WDF_USB_PIPE_INFORMATION info;
        WDFUSBPIPE pipe;

        WDF_USB_PIPE_INFORMATION_INIT(&info);
        pipe = WdfUsbInterfaceGetConfiguredPipe(ctx->Interface, i, &info);
        DbgPrint("qfdesk: pipe %u endpoint 0x%02x type %d max packet %lu\n", i, info.EndpointAddress,
                 (int) info.PipeType, info.MaximumPacketSize);
        WdfUsbTargetPipeSetNoMaximumPacketSizeCheck(pipe);
        if (info.PipeType == WdfUsbPipeTypeBulk && WdfUsbTargetPipeIsInEndpoint(pipe)) {
            ctx->BulkIn = pipe;
            ctx->Stats.bulk_in = info.EndpointAddress;
        }
        else if (info.PipeType == WdfUsbPipeTypeBulk && WdfUsbTargetPipeIsOutEndpoint(pipe)) {
            ctx->BulkOut = pipe;
            ctx->Stats.bulk_out = info.EndpointAddress;
        }
        else if (info.PipeType == WdfUsbPipeTypeInterrupt && WdfUsbTargetPipeIsInEndpoint(pipe)) {
            ctx->InterruptIn = pipe;
            ctx->Stats.interrupt_in = info.EndpointAddress;
        }
    }
    if (ctx->InterruptIn != NULL && !ctx->ReaderConfigured) {
        WDF_USB_CONTINUOUS_READER_CONFIG_INIT(&reader, EvtEventReadComplete, ctx, 8);
        reader.EvtUsbTargetPipeReadersFailed = EvtEventReadersFailed;
        status = WdfUsbTargetPipeConfigContinuousReader(ctx->InterruptIn, &reader);
        DbgPrint("qfdesk: WdfUsbTargetPipeConfigContinuousReader returned 0x%08lx\n", (ULONG) status);
        ctx->ReaderConfigured = NT_SUCCESS(status);
    }
    ctx->Stats.prepare_status = (ctx->BulkIn && ctx->BulkOut && ctx->ReaderConfigured) ? 0 : (ULONG) STATUS_DEVICE_CONFIGURATION_ERROR;
    return STATUS_SUCCESS;
}

static NTSTATUS EvtD0Entry(WDFDEVICE Device, WDF_POWER_DEVICE_STATE PreviousState)
{
    PDEVICE_CONTEXT ctx = GetDeviceContext(Device);
    NTSTATUS status = STATUS_SUCCESS;

    DbgPrint("qfdesk: EvtDeviceD0Entry from D%d\n", (int) PreviousState - 1);
    if (ctx->ReaderConfigured) {
        status = WdfIoTargetStart(WdfUsbTargetPipeGetIoTarget(ctx->InterruptIn));
        DbgPrint("qfdesk: interrupt pipe started 0x%08lx\n", (ULONG) status);
    }
    return STATUS_SUCCESS;
}

static NTSTATUS EvtD0Exit(WDFDEVICE Device, WDF_POWER_DEVICE_STATE TargetState)
{
    PDEVICE_CONTEXT ctx = GetDeviceContext(Device);

    DbgPrint("qfdesk: EvtDeviceD0Exit to D%d\n", (int) TargetState - 1);
    if (ctx->ReaderConfigured) {
        // Leave the pending reads: Wine's wineusb.sys cannot cancel them.
        WdfIoTargetStop(WdfUsbTargetPipeGetIoTarget(ctx->InterruptIn), WdfIoTargetLeaveSentIoPending);
    }
    return STATUS_SUCCESS;
}

static VOID SleepMs(ULONG Ms)
{
    LARGE_INTEGER interval;

    interval.QuadPart = -(LONGLONG) Ms * 10000;
    KeDelayExecutionThread(KernelMode, FALSE, &interval);
}

static VOID Checksum(const UCHAR *Data, ULONG Length, PUCHAR High, PUCHAR Low)
{
    ULONG s1 = 0x5A, s2 = 0, i;

    for (i = 0; i < Length; i++) {
        s1 = (s1 + Data[i]) % 251;
        s2 = (s2 + s1) % 251;
    }
    *High = (UCHAR) s2;
    *Low = (UCHAR) s1;
}

static NTSTATUS Control(PDEVICE_CONTEXT Ctx, WDF_USB_BMREQUEST_DIRECTION Direction, UCHAR Request, USHORT Value,
                        USHORT Index, PVOID Buffer, ULONG Length, PULONG Transferred)
{
    WDF_REQUEST_SEND_OPTIONS options;
    WDF_USB_CONTROL_SETUP_PACKET setup;
    WDF_MEMORY_DESCRIPTOR memory;

    WDF_REQUEST_SEND_OPTIONS_INIT(&options, 0);
    WDF_REQUEST_SEND_OPTIONS_SET_TIMEOUT(&options, WDF_REL_TIMEOUT_IN_SEC(5));
    WDF_USB_CONTROL_SETUP_PACKET_INIT_VENDOR(&setup, Direction, BmRequestToDevice, Request, Value, Index);
    if (Length == 0) {
        return WdfUsbTargetDeviceSendControlTransferSynchronously(Ctx->UsbDevice, NULL, &options, &setup, NULL,
                                                                  Transferred);
    }
    WDF_MEMORY_DESCRIPTOR_INIT_BUFFER(&memory, Buffer, Length);
    return WdfUsbTargetDeviceSendControlTransferSynchronously(Ctx->UsbDevice, NULL, &options, &setup, &memory,
                                                              Transferred);
}

static NTSTATUS Connect(PDEVICE_CONTEXT Ctx, struct qfdesk_hello *Hello)
{
    ULONG transferred = 0, nonce, response;
    NTSTATUS status;

    RtlZeroMemory(Hello, sizeof(*Hello));
    Ctx->Connected = FALSE;
    status = Control(Ctx, BmRequestDeviceToHost, 0x01, 0, 0, Hello->raw, sizeof(Hello->raw), &transferred);
    if (!NT_SUCCESS(status) || transferred != sizeof(Hello->raw)) {
        DbgPrint("qfdesk: connect: first step 0x%08lx, %lu bytes\n", (ULONG) status, transferred);
        return NT_SUCCESS(status) ? STATUS_DEVICE_PROTOCOL_ERROR : status;
    }
    nonce = (ULONG) Hello->raw[1] | ((ULONG) Hello->raw[2] << 8) | ((ULONG) Hello->raw[3] << 16) |
            ((ULONG) Hello->raw[4] << 24);
    response = ((nonce << 5) | (nonce >> 27)) ^ 0x6D2B79F5UL;
    status = Control(Ctx, BmRequestHostToDevice, 0x02, (USHORT) (response >> 16), (USHORT) (response & 0xFFFF), NULL,
                     0, &transferred);
    Hello->status = status;
    DbgPrint("qfdesk: connect 0x%08lx\n", (ULONG) status);
    if (!NT_SUCCESS(status)) {
        return status;
    }
    WdfSpinLockAcquire(Ctx->EventLock);
    Ctx->EventHead = Ctx->EventCount = 0;
    WdfSpinLockRelease(Ctx->EventLock);
    Ctx->Seq = 0;
    Ctx->Connected = TRUE;
    return STATUS_SUCCESS;
}

// Reads one response frame into Ctx->In and checks it. On success *Length
// is the frame's payload length (payload at Ctx->In + 4).
static NTSTATUS ReadFrame(PDEVICE_CONTEXT Ctx, UCHAR Seq, PULONG Length)
{
    WDF_REQUEST_SEND_OPTIONS options;
    WDF_MEMORY_DESCRIPTOR memory;
    ULONG transferred = 0;
    UCHAR high, low;
    NTSTATUS status;

    WDF_REQUEST_SEND_OPTIONS_INIT(&options, 0);
    WDF_REQUEST_SEND_OPTIONS_SET_TIMEOUT(&options, WDF_REL_TIMEOUT_IN_SEC(5));
    WDF_MEMORY_DESCRIPTOR_INIT_BUFFER(&memory, Ctx->In, FRAME_BUFFER);
    status = WdfUsbTargetPipeReadSynchronously(Ctx->BulkIn, NULL, &options, &memory, &transferred);
    if (!NT_SUCCESS(status)) {
        return status;
    }
    if (transferred < 6 || Ctx->In[0] != 0x9D || Ctx->In[1] != transferred - 4 || Ctx->In[2] != Seq) {
        DbgPrint("qfdesk: malformed response (%lu bytes)\n", transferred);
        return STATUS_DEVICE_PROTOCOL_ERROR;
    }
    Checksum(Ctx->In + 1, transferred - 3, &high, &low);
    if (Ctx->In[transferred - 2] != high || Ctx->In[transferred - 1] != low) {
        DbgPrint("qfdesk: response failed its check\n");
        return STATUS_CRC_ERROR;
    }
    *Length = transferred - 6;
    return STATUS_SUCCESS;
}

static NTSTATUS Transact(PDEVICE_CONTEXT Ctx, const struct qfdesk_command *Command, struct qfdesk_response *Response)
{
    WDF_REQUEST_SEND_OPTIONS options;
    WDF_MEMORY_DESCRIPTOR memory;
    ULONG transferred = 0, length = 0, frameLength, i, count;
    UCHAR seq;
    NTSTATUS status;

    RtlZeroMemory(Response, sizeof(*Response));
    if (!Ctx->Connected) {
        return STATUS_DEVICE_NOT_CONNECTED;
    }
    if (Command->length > QFDESK_MAX_PAYLOAD) {
        return STATUS_INVALID_PARAMETER;
    }
    Ctx->Seq++;
    if (Ctx->Seq == 0) {
        Ctx->Seq = 1;
    }
    seq = Ctx->Seq;
    Ctx->Out[0] = 0x9C;
    Ctx->Out[1] = (UCHAR) (Command->length + 2);
    Ctx->Out[2] = seq;
    Ctx->Out[3] = Command->cmd;
    RtlCopyMemory(Ctx->Out + 4, Command->payload, Command->length);
    Checksum(Ctx->Out + 1, Command->length + 3, &Ctx->Out[4 + Command->length], &Ctx->Out[5 + Command->length]);
    frameLength = Command->length + 6;

    WDF_REQUEST_SEND_OPTIONS_INIT(&options, 0);
    WDF_REQUEST_SEND_OPTIONS_SET_TIMEOUT(&options, WDF_REL_TIMEOUT_IN_SEC(5));
    WDF_MEMORY_DESCRIPTOR_INIT_BUFFER(&memory, Ctx->Out, frameLength);
    status = WdfUsbTargetPipeWriteSynchronously(Ctx->BulkOut, NULL, &options, &memory, &transferred);
    if (!NT_SUCCESS(status) || transferred != frameLength) {
        DbgPrint("qfdesk: write 0x%08lx, %lu of %lu bytes\n", (ULONG) status, transferred, frameLength);
        return NT_SUCCESS(status) ? STATUS_DEVICE_PROTOCOL_ERROR : status;
    }
    Ctx->Stats.transactions++;

    status = ReadFrame(Ctx, seq, &length);
    if (!NT_SUCCESS(status)) {
        return status;
    }
    Response->status = Ctx->In[3];
    Response->frames = 1;
    if (Response->status == 0x02 && length >= 2) {
        // A multi-frame response: the length and the number of frames that
        // follow, then that many frames, each starting with its index.
        count = Ctx->In[5];
        for (i = 0; i < count; i++) {
            status = ReadFrame(Ctx, seq, &length);
            if (!NT_SUCCESS(status)) {
                return status;
            }
            if (Ctx->In[3] != 0x03 || length < 1 || Ctx->In[4] != i ||
                Response->length + length - 1 > QFDESK_MAX_RESPONSE) {
                return STATUS_DEVICE_PROTOCOL_ERROR;
            }
            RtlCopyMemory(Response->payload + Response->length, Ctx->In + 5, length - 1);
            Response->length += (USHORT) (length - 1);
            Response->frames++;
        }
        Response->status = 0x00;
    }
    else {
        RtlCopyMemory(Response->payload, Ctx->In + 4, length);
        Response->length = (USHORT) length;
    }
    if (Command->cmd == 0x10 && Response->status == 0x00) {
        // The device ignores everything for a while after a mode change.
        SleepMs(250);
    }
    return STATUS_SUCCESS;
}

static VOID EvtIoDeviceControl(WDFQUEUE Queue, WDFREQUEST Request, size_t OutputBufferLength,
                               size_t InputBufferLength, ULONG IoControlCode)
{
    WDFDEVICE device = WdfIoQueueGetDevice(Queue);
    PDEVICE_CONTEXT ctx = GetDeviceContext(device);
    PVOID in = NULL, out = NULL;
    ULONG transferred = 0;
    ULONG_PTR information = 0;
    NTSTATUS status;

    UNREFERENCED_PARAMETER(OutputBufferLength);
    UNREFERENCED_PARAMETER(InputBufferLength);
    switch (IoControlCode) {
    case QFDESK_IOCTL_CONNECT:
        status = WdfRequestRetrieveOutputBuffer(Request, sizeof(struct qfdesk_hello), &out, NULL);
        if (NT_SUCCESS(status)) {
            status = Connect(ctx, (struct qfdesk_hello *) out);
            information = sizeof(struct qfdesk_hello);
        }
        break;

    case QFDESK_IOCTL_DISCONNECT:
        ctx->Connected = FALSE;
        status = Control(ctx, BmRequestHostToDevice, 0x03, 0, 0, NULL, 0, &transferred);
        break;

    case QFDESK_IOCTL_TRANSACT:
        status = WdfRequestRetrieveInputBuffer(Request, sizeof(struct qfdesk_command), &in, NULL);
        if (NT_SUCCESS(status)) {
            status = WdfRequestRetrieveOutputBuffer(Request, sizeof(struct qfdesk_response), &out, NULL);
        }
        if (NT_SUCCESS(status)) {
            // METHOD_BUFFERED: input and output are the same buffer.
            RtlCopyMemory(&ctx->Command, in, sizeof(struct qfdesk_command));
            status = Transact(ctx, &ctx->Command, (struct qfdesk_response *) out);
            information = sizeof(struct qfdesk_response);
        }
        if (!NT_SUCCESS(status)) {
            ctx->Stats.errors++;
        }
        break;

    case QFDESK_IOCTL_GET_EVENT:
        status = WdfRequestRetrieveOutputBuffer(Request, 8, &out, NULL);
        if (NT_SUCCESS(status)) {
            WdfSpinLockAcquire(ctx->EventLock);
            if (ctx->EventCount > 0) {
                RtlCopyMemory(out, ctx->Events[ctx->EventHead], 8);
                ctx->EventHead = (ctx->EventHead + 1) % EVENT_RING;
                ctx->EventCount--;
                information = 8;
            }
            WdfSpinLockRelease(ctx->EventLock);
        }
        break;

    case QFDESK_IOCTL_STATS:
        status = WdfRequestRetrieveOutputBuffer(Request, sizeof(struct qfdesk_stats), &out, NULL);
        if (NT_SUCCESS(status)) {
            RtlCopyMemory(out, &ctx->Stats, sizeof(struct qfdesk_stats));
            information = sizeof(struct qfdesk_stats);
        }
        break;

    default:
        status = STATUS_INVALID_DEVICE_REQUEST;
        break;
    }
    WdfRequestCompleteWithInformation(Request, status, information);
}
