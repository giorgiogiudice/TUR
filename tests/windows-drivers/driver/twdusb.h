// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// The interface between twdusb.sys (the TUR test WDM USB function driver) and
// the programs that talk to it (twdtest.exe). Included by both; it needs
// CTL_CODE, GUID and the fixed-size integer types of the Windows headers.
//
// Everything here is ours: the GUID was generated for this test, the IOCTL
// numbers are in the range Microsoft leaves to vendors (function >= 0x800).
#pragma once

// {6c1b8a52-7a0f-4d0c-9a8e-2f4b1d3e5a71}
#define TWD_INTERFACE_GUID_INIT \
    { 0x6c1b8a52, 0x7a0f, 0x4d0c, { 0x9a, 0x8e, 0x2f, 0x4b, 0x1d, 0x3e, 0x5a, 0x71 } }

#define TWD_IOCTL(n) CTL_CODE(FILE_DEVICE_UNKNOWN, 0x800 + (n), METHOD_BUFFERED, FILE_ANY_ACCESS)
#define TWD_IOCTL_INFO TWD_IOCTL(0)
#define TWD_IOCTL_TRANSFER TWD_IOCTL(1)

#define TWD_VERSION 1
#define TWD_MAX_PIPES 8

// Steps of IRP_MN_START_DEVICE; info.start_step is the last one reached.
#define TWD_STEP_FORWARD 1      // START_DEVICE passed down to the bus PDO
#define TWD_STEP_DEVICE_DESC 2  // GET_DESCRIPTOR (device)
#define TWD_STEP_CONFIG_HEAD 3  // GET_DESCRIPTOR (configuration, first 9 bytes)
#define TWD_STEP_CONFIG_FULL 4  // GET_DESCRIPTOR (configuration, wTotalLength)
#define TWD_STEP_SELECT 5       // SELECT_CONFIGURATION
#define TWD_STEP_DONE 6

#pragma pack(push, 1)
struct twd_pipe {
    UCHAR endpoint;  // bEndpointAddress
    UCHAR type;      // USBD_PIPE_TYPE: 0 control, 1 isochronous, 2 bulk, 3 interrupt
    USHORT max_packet;
};

struct twd_info {
    ULONG version;
    LONG start_status;  // NTSTATUS of the last start step
    ULONG start_step;
    USHORT vendor, product, bcd_device, bcd_usb;
    UCHAR device_class;
    UCHAR num_configurations;
    UCHAR num_interfaces;  // of the configuration descriptor
    UCHAR interface_class; // of the first interface descriptor
    USHORT config_total_length;
    UCHAR num_pipes;
    UCHAR reserved;
    struct twd_pipe pipes[TWD_MAX_PIPES];
    ULONG urbs_sent;
    ULONG transfers_done;
    char tag[16];  // "twdusb-wdm-1"
};

#define TWD_KIND_PIPE 1    // bulk or interrupt transfer on endpoint `endpoint`
#define TWD_KIND_VENDOR 2  // vendor control transfer to the device

#define TWD_DIR_OUT 0
#define TWD_DIR_IN 1

// How a timed-out transfer was ended.
#define TWD_CANCEL_NONE 0         // no time-out
#define TWD_CANCEL_IOCANCELIRP 1  // IoCancelIrp ended it
#define TWD_CANCEL_ABORT_PIPE 2   // IoCancelIrp did not; URB_FUNCTION_ABORT_PIPE did
#define TWD_CANCEL_STUCK 3        // neither, within the waits below; waited on

struct twd_transfer {
    ULONG kind;
    ULONG direction;  // for TWD_KIND_VENDOR; pipes take it from the endpoint
    UCHAR endpoint;
    UCHAR request;
    USHORT value;
    USHORT index;
    USHORT reserved;
    ULONG length;      // bytes to send (data follows) or to receive
    ULONG timeout_ms;  // 0: wait for ever
    // OUT data follows
};

struct twd_result {
    LONG status;        // NTSTATUS of the URB's IRP
    LONG usbd_status;   // URB header status
    ULONG length;       // bytes moved
    ULONG cancel_path;  // TWD_CANCEL_*
    ULONG elapsed_us;
    // IN data follows
};
#pragma pack(pop)
