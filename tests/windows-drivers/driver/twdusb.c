// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// twdusb.sys: the smallest useful WDM USB function driver we could write, for
// the windows-drivers experiment (docs/reports/windows-drivers-1.md).
//
// It is a Windows kernel-mode driver in form only: a PE image with the native
// subsystem that imports ntoskrnl.exe and usbd.sys. In this experiment it is
// only ever loaded by Wine's winedevice.exe, an ordinary Linux user-space
// process. It is never loaded into any kernel.
//
// What it does, all of it the classic WDM way and nothing newer, so that a
// failure points at Wine's USB stack and not at a framework:
//   AddDevice     create an FDO, attach it to the bus PDO, register a device
//                 interface (twdusb.h) so that a program can find it.
//   START_DEVICE  pass it down, then read the device and configuration
//                 descriptors and select configuration 1 with
//                 USBD_CreateConfigurationRequestEx; keep the pipe handles.
//                 Every step is recorded; a failed step does not fail the
//                 start, so the test program can still ask which step failed.
//   DEVICE_CONTROL
//                 TWD_IOCTL_INFO: what START_DEVICE found.
//                 TWD_IOCTL_TRANSFER: one bulk/interrupt transfer on a pipe
//                 or one vendor control transfer to the device, with a
//                 time-out. On time-out the driver tries IoCancelIrp first
//                 and URB_FUNCTION_ABORT_PIPE second and reports which one
//                 ended the transfer.
// Every URB goes down as IOCTL_INTERNAL_USB_SUBMIT_URB in an IRP the driver
// allocates itself and frees after its completion routine has run.
//
// Kernel APIs deliberately avoided because Wine's user-space ntoskrnl cannot
// give them their kernel meaning (see the report): KeGetCurrentIrql (reads
// CR8 inline on x86-64), KeQuerySystemTime (reads the kernel's shared-data
// page inline), spin locks, DPCs, IoForwardIrpSynchronously (a stub in Wine).
#include <ddk/wdm.h>
#include <ddk/usbdrivr.h>

#include "twdusb.h"

#define TWD_TAG 0x44575454 /* 'TTWD' */

static const GUID twd_interface_guid = TWD_INTERFACE_GUID_INIT;

struct twd_pipe_state {
    USBD_PIPE_HANDLE handle;
    UCHAR endpoint;
    UCHAR type;
    USHORT max_packet;
};

struct twd_ext {
    DEVICE_OBJECT *self;
    DEVICE_OBJECT *pdo;
    DEVICE_OBJECT *lower;
    UNICODE_STRING interface_name;
    BOOLEAN interface_registered;
    NTSTATUS start_status;
    ULONG start_step;
    USB_DEVICE_DESCRIPTOR device_desc;
    USB_CONFIGURATION_DESCRIPTOR *config_desc;
    UCHAR num_interfaces;
    UCHAR interface_class;
    ULONG num_pipes;
    struct twd_pipe_state pipes[TWD_MAX_PIPES];
    LONG urbs_sent;
    LONG transfers_done;
};

static NTSTATUS NTAPI twd_signal(DEVICE_OBJECT *device, IRP *irp, void *context)
{
    (void)device;
    (void)irp;
    KeSetEvent((KEVENT *)context, IO_NO_INCREMENT, FALSE);
    return STATUS_MORE_PROCESSING_REQUIRED;
}

static NTSTATUS twd_wait(KEVENT *event, ULONG timeout_ms)
{
    LARGE_INTEGER timeout;

    if (!timeout_ms)
        return KeWaitForSingleObject(event, Executive, KernelMode, FALSE, NULL);
    timeout.QuadPart = -10000LL * (LONGLONG)timeout_ms;
    return KeWaitForSingleObject(event, Executive, KernelMode, FALSE, &timeout);
}

static NTSTATUS twd_abort_pipe(struct twd_ext *ext, USBD_PIPE_HANDLE pipe);

// Send one URB down and wait for it. pipe is the pipe a transfer is on, for
// URB_FUNCTION_ABORT_PIPE after a time-out; NULL for control transfers.
//
// When neither IoCancelIrp nor ABORT_PIPE ends a timed-out transfer, the
// IRP, its event, the URB and the URB's buffer still belong to the bus
// driver, which may complete the IRP at any later time. The driver then
// leaks all four on purpose (cancel_path is TWD_CANCEL_STUCK and the caller
// must not free the URB or its buffer) and returns STATUS_IO_TIMEOUT, so the
// program asking is not blocked for ever and can report what happened.
static NTSTATUS twd_send_urb(struct twd_ext *ext, URB *urb, ULONG timeout_ms,
                             USBD_PIPE_HANDLE pipe, ULONG *cancel_path)
{
    IO_STACK_LOCATION *next;
    NTSTATUS status;
    KEVENT *event;
    IRP *irp;

    *cancel_path = TWD_CANCEL_NONE;
    if (!(event = ExAllocatePoolWithTag(NonPagedPool, sizeof(*event), TWD_TAG)))
        return STATUS_INSUFFICIENT_RESOURCES;
    if (!(irp = IoAllocateIrp(ext->lower->StackSize, FALSE))) {
        ExFreePoolWithTag(event, TWD_TAG);
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    KeInitializeEvent(event, NotificationEvent, FALSE);
    irp->IoStatus.Status = STATUS_NOT_SUPPORTED;
    next = IoGetNextIrpStackLocation(irp);
    next->MajorFunction = IRP_MJ_INTERNAL_DEVICE_CONTROL;
    next->Parameters.DeviceIoControl.IoControlCode = IOCTL_INTERNAL_USB_SUBMIT_URB;
    next->Parameters.Others.Argument1 = urb;
    IoSetCompletionRoutine(irp, twd_signal, event, TRUE, TRUE, TRUE);

    InterlockedIncrement(&ext->urbs_sent);
    status = IoCallDriver(ext->lower, irp);
    if (status == STATUS_PENDING && twd_wait(event, timeout_ms) == STATUS_TIMEOUT) {
        // The IRP is ours until the completion routine has run, so it may be
        // cancelled here whatever the bus driver is doing.
        IoCancelIrp(irp);
        if (twd_wait(event, 2000) != STATUS_TIMEOUT) {
            *cancel_path = TWD_CANCEL_IOCANCELIRP;
        } else {
            if (pipe)
                twd_abort_pipe(ext, pipe);
            if (pipe && twd_wait(event, 5000) != STATUS_TIMEOUT) {
                *cancel_path = TWD_CANCEL_ABORT_PIPE;
            } else {
                *cancel_path = TWD_CANCEL_STUCK;
                DbgPrint("twdusb: URB function %#x stuck in the bus driver; leaking it\n",
                         urb->UrbHeader.Function);
                return STATUS_IO_TIMEOUT;
            }
        }
    }
    status = irp->IoStatus.Status;
    IoFreeIrp(irp);
    ExFreePoolWithTag(event, TWD_TAG);
    return status;
}

static NTSTATUS twd_abort_pipe(struct twd_ext *ext, USBD_PIPE_HANDLE pipe)
{
    struct _URB_PIPE_REQUEST *req;
    ULONG path;
    NTSTATUS status;

    if (!(req = ExAllocatePoolWithTag(NonPagedPool, sizeof(*req), TWD_TAG)))
        return STATUS_INSUFFICIENT_RESOURCES;
    RtlZeroMemory(req, sizeof(*req));
    req->Hdr.Length = sizeof(*req);
    req->Hdr.Function = URB_FUNCTION_ABORT_PIPE;
    req->PipeHandle = pipe;
    status = twd_send_urb(ext, (URB *)req, 5000, NULL, &path);
    if (path != TWD_CANCEL_STUCK)
        ExFreePoolWithTag(req, TWD_TAG);
    return status;
}

static NTSTATUS twd_get_descriptor(struct twd_ext *ext, UCHAR type, void *buffer, ULONG length,
                                   ULONG *returned)
{
    ULONG path;
    URB *urb;
    NTSTATUS status;

    if (!(urb = ExAllocatePoolWithTag(NonPagedPool, sizeof(URB), TWD_TAG)))
        return STATUS_INSUFFICIENT_RESOURCES;
    RtlZeroMemory(urb, sizeof(URB));
    UsbBuildGetDescriptorRequest(urb, (USHORT)sizeof(struct _URB_CONTROL_DESCRIPTOR_REQUEST), type,
                                 0, 0, buffer, NULL, length, NULL);
    status = twd_send_urb(ext, urb, 5000, NULL, &path);
    if (path == TWD_CANCEL_STUCK)
        return status; // urb and buffer stay with the bus driver; see twd_send_urb
    if (NT_SUCCESS(status) && !USBD_SUCCESS(urb->UrbHeader.Status))
        status = STATUS_UNSUCCESSFUL;
    if (returned)
        *returned = urb->UrbControlDescriptorRequest.TransferBufferLength;
    ExFreePoolWithTag(urb, TWD_TAG);
    return status;
}

static NTSTATUS twd_configure(struct twd_ext *ext)
{
    USB_CONFIGURATION_DESCRIPTOR head;
    USBD_INTERFACE_LIST_ENTRY list[2];
    USB_INTERFACE_DESCRIPTOR *iface;
    USBD_INTERFACE_INFORMATION *info;
    NTSTATUS status;
    ULONG got = 0, i, path;
    URB *urb;

    ext->start_step = TWD_STEP_DEVICE_DESC;
    RtlZeroMemory(&ext->device_desc, sizeof(ext->device_desc));
    status = twd_get_descriptor(ext, USB_DEVICE_DESCRIPTOR_TYPE, &ext->device_desc,
                                sizeof(ext->device_desc), &got);
    if (!NT_SUCCESS(status))
        return status;
    if (got < sizeof(ext->device_desc))
        return STATUS_DEVICE_DATA_ERROR;

    ext->start_step = TWD_STEP_CONFIG_HEAD;
    RtlZeroMemory(&head, sizeof(head));
    status = twd_get_descriptor(ext, USB_CONFIGURATION_DESCRIPTOR_TYPE, &head, sizeof(head), &got);
    if (!NT_SUCCESS(status))
        return status;
    if (got < sizeof(head) || head.wTotalLength < sizeof(head))
        return STATUS_DEVICE_DATA_ERROR;

    ext->start_step = TWD_STEP_CONFIG_FULL;
    if (!(ext->config_desc = ExAllocatePoolWithTag(NonPagedPool, head.wTotalLength, TWD_TAG)))
        return STATUS_INSUFFICIENT_RESOURCES;
    RtlZeroMemory(ext->config_desc, head.wTotalLength);
    status = twd_get_descriptor(ext, USB_CONFIGURATION_DESCRIPTOR_TYPE, ext->config_desc,
                                head.wTotalLength, &got);
    if (!NT_SUCCESS(status))
        return status;
    if (got < head.wTotalLength)
        return STATUS_DEVICE_DATA_ERROR;
    ext->num_interfaces = ext->config_desc->bNumInterfaces;

    ext->start_step = TWD_STEP_SELECT;
    // The first interface, alternate setting 0: all the test devices need.
    iface = USBD_ParseConfigurationDescriptorEx(ext->config_desc, ext->config_desc, -1, 0, -1, -1, -1);
    if (!iface)
        return STATUS_DEVICE_CONFIGURATION_ERROR;
    ext->interface_class = iface->bInterfaceClass;
    list[0].InterfaceDescriptor = iface;
    list[0].Interface = NULL;
    list[1].InterfaceDescriptor = NULL;
    list[1].Interface = NULL;
    if (!(urb = USBD_CreateConfigurationRequestEx(ext->config_desc, list)))
        return STATUS_INSUFFICIENT_RESOURCES;
    status = twd_send_urb(ext, urb, 5000, NULL, &path);
    if (path == TWD_CANCEL_STUCK)
        return status;
    if (NT_SUCCESS(status) && !USBD_SUCCESS(urb->UrbHeader.Status))
        status = STATUS_UNSUCCESSFUL;
    if (NT_SUCCESS(status)) {
        info = &urb->UrbSelectConfiguration.Interface;
        ext->num_pipes = info->NumberOfPipes;
        if (ext->num_pipes > TWD_MAX_PIPES)
            ext->num_pipes = TWD_MAX_PIPES;
        for (i = 0; i < ext->num_pipes; i++) {
            ext->pipes[i].handle = info->Pipes[i].PipeHandle;
            ext->pipes[i].endpoint = info->Pipes[i].EndpointAddress;
            ext->pipes[i].type = (UCHAR)info->Pipes[i].PipeType;
            ext->pipes[i].max_packet = info->Pipes[i].MaximumPacketSize;
        }
        ext->start_step = TWD_STEP_DONE;
    }
    ExFreePool(urb);
    return status;
}

static NTSTATUS twd_complete(IRP *irp, NTSTATUS status, ULONG_PTR information)
{
    irp->IoStatus.Status = status;
    irp->IoStatus.Information = information;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    return status;
}

static NTSTATUS twd_forward_and_wait(struct twd_ext *ext, IRP *irp)
{
    KEVENT event;
    NTSTATUS status;

    KeInitializeEvent(&event, NotificationEvent, FALSE);
    IoCopyCurrentIrpStackLocationToNext(irp);
    IoSetCompletionRoutine(irp, twd_signal, &event, TRUE, TRUE, TRUE);
    status = IoCallDriver(ext->lower, irp);
    if (status == STATUS_PENDING) {
        twd_wait(&event, 0);
        status = irp->IoStatus.Status;
    }
    return status;
}

static NTSTATUS NTAPI twd_pnp(DEVICE_OBJECT *device, IRP *irp)
{
    struct twd_ext *ext = device->DeviceExtension;
    IO_STACK_LOCATION *stack = IoGetCurrentIrpStackLocation(irp);
    NTSTATUS status;

    switch (stack->MinorFunction) {
    case IRP_MN_START_DEVICE:
        ext->start_step = TWD_STEP_FORWARD;
        status = twd_forward_and_wait(ext, irp);
        if (NT_SUCCESS(status))
            status = twd_configure(ext);
        ext->start_status = status;
        DbgPrint("twdusb: start step %lu status %#lx\n", ext->start_step, (ULONG)status);
        if (ext->interface_registered)
            IoSetDeviceInterfaceState(&ext->interface_name, TRUE);
        // Started whatever configure found, so that the test program can
        // open the device and read which step failed.
        return twd_complete(irp, STATUS_SUCCESS, 0);

    case IRP_MN_SURPRISE_REMOVAL:
        if (ext->interface_registered)
            IoSetDeviceInterfaceState(&ext->interface_name, FALSE);
        irp->IoStatus.Status = STATUS_SUCCESS;
        IoSkipCurrentIrpStackLocation(irp);
        return IoCallDriver(ext->lower, irp);

    case IRP_MN_REMOVE_DEVICE:
        if (ext->interface_registered) {
            IoSetDeviceInterfaceState(&ext->interface_name, FALSE);
            RtlFreeUnicodeString(&ext->interface_name);
            ext->interface_registered = FALSE;
        }
        irp->IoStatus.Status = STATUS_SUCCESS;
        IoSkipCurrentIrpStackLocation(irp);
        status = IoCallDriver(ext->lower, irp);
        if (ext->config_desc)
            ExFreePoolWithTag(ext->config_desc, TWD_TAG);
        IoDetachDevice(ext->lower);
        IoDeleteDevice(device);
        DbgPrint("twdusb: removed\n");
        return status;

    default:
        IoSkipCurrentIrpStackLocation(irp);
        return IoCallDriver(ext->lower, irp);
    }
}

static NTSTATUS twd_ioctl_info(struct twd_ext *ext, IRP *irp, ULONG out_length)
{
    struct twd_info *info = irp->AssociatedIrp.SystemBuffer;
    ULONG i;

    if (out_length < sizeof(*info))
        return twd_complete(irp, STATUS_BUFFER_TOO_SMALL, 0);
    RtlZeroMemory(info, sizeof(*info));
    info->version = TWD_VERSION;
    info->start_status = ext->start_status;
    info->start_step = ext->start_step;
    info->vendor = ext->device_desc.idVendor;
    info->product = ext->device_desc.idProduct;
    info->bcd_device = ext->device_desc.bcdDevice;
    info->bcd_usb = ext->device_desc.bcdUSB;
    info->device_class = ext->device_desc.bDeviceClass;
    info->num_configurations = ext->device_desc.bNumConfigurations;
    info->num_interfaces = ext->num_interfaces;
    info->interface_class = ext->interface_class;
    info->config_total_length = ext->config_desc ? ext->config_desc->wTotalLength : 0;
    info->num_pipes = (UCHAR)ext->num_pipes;
    for (i = 0; i < ext->num_pipes; i++) {
        info->pipes[i].endpoint = ext->pipes[i].endpoint;
        info->pipes[i].type = ext->pipes[i].type;
        info->pipes[i].max_packet = ext->pipes[i].max_packet;
    }
    info->urbs_sent = (ULONG)ext->urbs_sent;
    info->transfers_done = (ULONG)ext->transfers_done;
    RtlCopyMemory(info->tag, "twdusb-wdm-1", sizeof("twdusb-wdm-1"));
    return twd_complete(irp, STATUS_SUCCESS, sizeof(*info));
}

static NTSTATUS twd_ioctl_transfer(struct twd_ext *ext, IRP *irp, ULONG in_length, ULONG out_length)
{
    UCHAR *system = irp->AssociatedIrp.SystemBuffer;
    struct twd_transfer request;
    struct twd_result *result;
    struct twd_pipe_state *pipe = NULL;
    ULONG cancel_path = TWD_CANCEL_NONE, i, moved = 0;
    BOOLEAN in;
    NTSTATUS status;
    UCHAR *buffer;
    URB *urb;

    if (in_length < sizeof(request) || out_length < sizeof(*result))
        return twd_complete(irp, STATUS_BUFFER_TOO_SMALL, 0);
    RtlCopyMemory(&request, system, sizeof(request));
    if (request.length > 65536)
        return twd_complete(irp, STATUS_INVALID_PARAMETER, 0);

    if (request.kind == TWD_KIND_PIPE) {
        for (i = 0; i < ext->num_pipes; i++)
            if (ext->pipes[i].endpoint == request.endpoint)
                pipe = &ext->pipes[i];
        if (!pipe)
            return twd_complete(irp, STATUS_INVALID_PARAMETER, 0);
        in = (request.endpoint & 0x80) != 0;
    } else if (request.kind == TWD_KIND_VENDOR) {
        in = request.direction == TWD_DIR_IN;
    } else {
        return twd_complete(irp, STATUS_INVALID_PARAMETER, 0);
    }
    if (in && out_length < sizeof(*result) + request.length)
        return twd_complete(irp, STATUS_BUFFER_TOO_SMALL, 0);
    if (!in && in_length < sizeof(request) + request.length)
        return twd_complete(irp, STATUS_INVALID_PARAMETER, 0);

    if (!(buffer = ExAllocatePoolWithTag(NonPagedPool, request.length ? request.length : 1, TWD_TAG)))
        return twd_complete(irp, STATUS_INSUFFICIENT_RESOURCES, 0);
    if (!(urb = ExAllocatePoolWithTag(NonPagedPool, sizeof(URB), TWD_TAG))) {
        ExFreePoolWithTag(buffer, TWD_TAG);
        return twd_complete(irp, STATUS_INSUFFICIENT_RESOURCES, 0);
    }
    RtlZeroMemory(urb, sizeof(URB));
    if (!in)
        RtlCopyMemory(buffer, system + sizeof(request), request.length);
    else
        RtlZeroMemory(buffer, request.length ? request.length : 1);

    if (request.kind == TWD_KIND_PIPE) {
        UsbBuildInterruptOrBulkTransferRequest(
            urb, (USHORT)sizeof(struct _URB_BULK_OR_INTERRUPT_TRANSFER), pipe->handle, buffer, NULL,
            request.length, (in ? USBD_TRANSFER_DIRECTION_IN : USBD_TRANSFER_DIRECTION_OUT) | USBD_SHORT_TRANSFER_OK,
            NULL);
        status = twd_send_urb(ext, urb, request.timeout_ms, pipe->handle, &cancel_path);
        moved = urb->UrbBulkOrInterruptTransfer.TransferBufferLength;
    } else {
        UsbBuildVendorRequest(urb, URB_FUNCTION_VENDOR_DEVICE,
                              (USHORT)sizeof(struct _URB_CONTROL_VENDOR_OR_CLASS_REQUEST),
                              in ? USBD_TRANSFER_DIRECTION_IN : USBD_TRANSFER_DIRECTION_OUT, 0,
                              request.request, request.value, request.index, buffer, NULL,
                              request.length, NULL);
        status = twd_send_urb(ext, urb, request.timeout_ms, NULL, &cancel_path);
        moved = urb->UrbControlVendorClassRequest.TransferBufferLength;
    }
    if (moved > request.length)
        moved = request.length;
    InterlockedIncrement(&ext->transfers_done);

    result = (struct twd_result *)system;
    RtlZeroMemory(result, sizeof(*result));
    result->status = status;
    result->usbd_status = urb->UrbHeader.Status;
    result->length = moved;
    result->cancel_path = cancel_path;
    if (in && moved)
        RtlCopyMemory(system + sizeof(*result), buffer, moved);

    if (cancel_path != TWD_CANCEL_STUCK) {
        ExFreePoolWithTag(urb, TWD_TAG);
        ExFreePoolWithTag(buffer, TWD_TAG);
    }
    // The IOCTL itself succeeds; the transfer's own result is in the reply.
    return twd_complete(irp, STATUS_SUCCESS, sizeof(*result) + (in ? moved : 0));
}

static NTSTATUS NTAPI twd_device_control(DEVICE_OBJECT *device, IRP *irp)
{
    struct twd_ext *ext = device->DeviceExtension;
    IO_STACK_LOCATION *stack = IoGetCurrentIrpStackLocation(irp);
    ULONG in_length = stack->Parameters.DeviceIoControl.InputBufferLength;
    ULONG out_length = stack->Parameters.DeviceIoControl.OutputBufferLength;

    switch (stack->Parameters.DeviceIoControl.IoControlCode) {
    case TWD_IOCTL_INFO:
        return twd_ioctl_info(ext, irp, out_length);
    case TWD_IOCTL_TRANSFER:
        if (ext->start_step != TWD_STEP_DONE)
            return twd_complete(irp, STATUS_DEVICE_NOT_READY, 0);
        return twd_ioctl_transfer(ext, irp, in_length, out_length);
    default:
        return twd_complete(irp, STATUS_INVALID_DEVICE_REQUEST, 0);
    }
}

static NTSTATUS NTAPI twd_create_close(DEVICE_OBJECT *device, IRP *irp)
{
    (void)device;
    return twd_complete(irp, STATUS_SUCCESS, 0);
}

static NTSTATUS NTAPI twd_pass_down(DEVICE_OBJECT *device, IRP *irp)
{
    struct twd_ext *ext = device->DeviceExtension;
    IoSkipCurrentIrpStackLocation(irp);
    return IoCallDriver(ext->lower, irp);
}

static NTSTATUS NTAPI twd_power(DEVICE_OBJECT *device, IRP *irp)
{
    struct twd_ext *ext = device->DeviceExtension;
    PoStartNextPowerIrp(irp);
    IoSkipCurrentIrpStackLocation(irp);
    return PoCallDriver(ext->lower, irp);
}

static NTSTATUS NTAPI twd_add_device(DRIVER_OBJECT *driver, DEVICE_OBJECT *pdo)
{
    DEVICE_OBJECT *fdo;
    struct twd_ext *ext;
    NTSTATUS status;

    status = IoCreateDevice(driver, sizeof(struct twd_ext), NULL, FILE_DEVICE_UNKNOWN,
                            FILE_DEVICE_SECURE_OPEN, FALSE, &fdo);
    if (!NT_SUCCESS(status))
        return status;
    ext = fdo->DeviceExtension;
    RtlZeroMemory(ext, sizeof(*ext));
    ext->self = fdo;
    ext->pdo = pdo;
    if (!(ext->lower = IoAttachDeviceToDeviceStack(fdo, pdo))) {
        IoDeleteDevice(fdo);
        return STATUS_NO_SUCH_DEVICE;
    }
    status = IoRegisterDeviceInterface(pdo, &twd_interface_guid, NULL, &ext->interface_name);
    ext->interface_registered = NT_SUCCESS(status);
    DbgPrint("twdusb: AddDevice pdo %p interface status %#lx\n", pdo, (ULONG)status);
    fdo->Flags |= DO_BUFFERED_IO | DO_POWER_PAGABLE;
    fdo->Flags &= ~DO_DEVICE_INITIALIZING;
    return STATUS_SUCCESS;
}

static void NTAPI twd_unload(DRIVER_OBJECT *driver)
{
    (void)driver;
    DbgPrint("twdusb: unload\n");
}

NTSTATUS NTAPI DriverEntry(DRIVER_OBJECT *driver, UNICODE_STRING *registry_path)
{
    (void)registry_path;
    DbgPrint("twdusb: DriverEntry\n");
    driver->DriverExtension->AddDevice = twd_add_device;
    driver->DriverUnload = twd_unload;
    driver->MajorFunction[IRP_MJ_CREATE] = twd_create_close;
    driver->MajorFunction[IRP_MJ_CLOSE] = twd_create_close;
    driver->MajorFunction[IRP_MJ_DEVICE_CONTROL] = twd_device_control;
    driver->MajorFunction[IRP_MJ_PNP] = twd_pnp;
    driver->MajorFunction[IRP_MJ_POWER] = twd_power;
    driver->MajorFunction[IRP_MJ_SYSTEM_CONTROL] = twd_pass_down;
    return STATUS_SUCCESS;
}
