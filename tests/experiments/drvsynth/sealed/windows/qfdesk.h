// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// SEALED (driver-synthesis experiment): the interface between qfdesk.sys,
// the KMDF driver of the invented desk device, and gadget.exe, its
// command-line program. The device protocol itself is in
// ../PROTOCOL.md; the driver owns the handshake, framing, checksum,
// sequence numbers, multi-frame responses, the mode-switch delay and the
// event reports, and the program maps user commands onto commands.
#pragma once

// {3a8e5c41-7b2d-4f19-a0c6-51464431c0de}
#define QFDESK_INTERFACE_GUID_INIT \
    { 0x3a8e5c41, 0x7b2d, 0x4f19, { 0xa0, 0xc6, 0x51, 0x46, 0x44, 0x31, 0xc0, 0xde } }

#define QFDESK_DEVICE_TYPE 0x8f10
#define QFDESK_IOCTL(n) (((ULONG)QFDESK_DEVICE_TYPE << 16) | ((ULONG)(n) << 2)) // METHOD_BUFFERED, FILE_ANY_ACCESS

// Out: struct qfdesk_hello. HELLO, the response to the nonce, AUTH.
#define QFDESK_IOCTL_CONNECT    QFDESK_IOCTL(0x910)
// BYE.
#define QFDESK_IOCTL_DISCONNECT QFDESK_IOCTL(0x911)
// In: struct qfdesk_command; out: struct qfdesk_response.
#define QFDESK_IOCTL_TRANSACT   QFDESK_IOCTL(0x912)
// Out: one 8-byte event report, or nothing (0 bytes) when none is queued.
#define QFDESK_IOCTL_GET_EVENT  QFDESK_IOCTL(0x913)
// Out: struct qfdesk_stats.
#define QFDESK_IOCTL_STATS      QFDESK_IOCTL(0x914)

#define QFDESK_MAX_PAYLOAD 48
#define QFDESK_MAX_RESPONSE 256

#pragma pack(push, 1)
struct qfdesk_hello {
    unsigned char raw[8];
    long status;  // NTSTATUS of AUTH
};

struct qfdesk_command {
    unsigned char cmd;
    unsigned char length;
    unsigned char payload[QFDESK_MAX_PAYLOAD];
};

struct qfdesk_response {
    unsigned char status;   // the device's status byte
    unsigned char frames;   // response frames read
    unsigned short length;  // bytes in payload (multi-frame responses concatenated)
    unsigned char payload[QFDESK_MAX_RESPONSE];
};

struct qfdesk_stats {
    char tag[8];  // "QFDESK"
    unsigned long prepare_status;
    unsigned char bulk_in, bulk_out, interrupt_in, pipes;
    unsigned long events_received, events_dropped, transactions, errors;
};
#pragma pack(pop)
