// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// twdtest.exe: the application side of the windows-drivers experiment. It
// finds a device that twdusb.sys drives, through the device interface the
// driver registers (SetupDiGetClassDevs + CreateFile, as any Windows vendor
// tool does), and exchanges data with it through the driver's IOCTLs.
//
//   twdtest.exe info       VID PID WAIT_SECONDS   find the device, print what the driver found
//   twdtest.exe loopback   VID PID WAIT_SECONDS   Linux f_loopback gadget: bulk OUT echoed on bulk IN
//   twdtest.exe sourcesink VID PID WAIT_SECONDS   Linux f_sourcesink gadget: vendor control
//                                                 requests 0x5b/0x5c and patterned bulk data
//   twdtest.exe interrupt  VID PID WAIT_SECONDS   read one interrupt-IN transfer (the HID gadget,
//                                                 as a raw USB device)
//   twdtest.exe soak       VID PID WAIT_SECONDS SECONDS
//                                                 64-byte loopback echoes for SECONDS; reports
//                                                 when they stopped working, if they did
//
// Output: "TWD info ...", "TWD metric NAME VALUE" and "TWD check NAME PASS|FAIL
// DETAIL" lines. Exit status: 0 every check passed, 1 a check failed, 3 no
// device was found within WAIT_SECONDS.
#include <windows.h>
#include <winioctl.h>
#include <setupapi.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "../driver/twdusb.h"

static const GUID twd_guid = TWD_INTERFACE_GUID_INIT;
static int failures;

static void check(const char *name, int ok, const char *fmt, ...)
{
    va_list args;
    printf("TWD check %s %s ", name, ok ? "PASS" : "FAIL");
    va_start(args, fmt);
    vprintf(fmt, args);
    va_end(args);
    printf("\n");
    if (!ok)
        failures++;
}

static double now_seconds(void)
{
    LARGE_INTEGER counter, frequency;
    QueryPerformanceCounter(&counter);
    QueryPerformanceFrequency(&frequency);
    return (double)counter.QuadPart / (double)frequency.QuadPart;
}

static int get_info(HANDLE device, struct twd_info *info)
{
    DWORD returned = 0;
    memset(info, 0, sizeof(*info));
    return DeviceIoControl(device, TWD_IOCTL_INFO, NULL, 0, info, sizeof(*info), &returned, NULL) &&
           returned >= sizeof(*info);
}

static void print_info(const struct twd_info *info)
{
    int i;
    printf("TWD info tag=%.16s version=%lu start_step=%lu start_status=%#lx vid=%04x pid=%04x bcdDevice=%04x "
           "bcdUSB=%04x class=%02x configs=%u interfaces=%u interface_class=%02x config_length=%u pipes=%u "
           "urbs=%lu\n",
           info->tag, info->version, info->start_step, (unsigned long)info->start_status, info->vendor,
           info->product, info->bcd_device, info->bcd_usb, info->device_class, info->num_configurations,
           info->num_interfaces, info->interface_class, info->config_total_length, info->num_pipes,
           info->urbs_sent);
    for (i = 0; i < info->num_pipes && i < TWD_MAX_PIPES; i++)
        printf("TWD info pipe endpoint=%02x type=%u max_packet=%u\n", info->pipes[i].endpoint,
               info->pipes[i].type, info->pipes[i].max_packet);
}

// Find the twdusb device with this VID/PID, waiting up to wait_seconds.
static HANDLE find_device(unsigned vid, unsigned pid, int wait_seconds, struct twd_info *found)
{
    double deadline = now_seconds() + wait_seconds;
    int printed_none = 0;

    for (;;) {
        HDEVINFO set = SetupDiGetClassDevsW(&twd_guid, NULL, NULL, DIGCF_DEVICEINTERFACE | DIGCF_PRESENT);
        SP_DEVICE_INTERFACE_DATA iface = {sizeof(iface)};
        DWORD i, seen = 0;

        if (set != INVALID_HANDLE_VALUE) {
            for (i = 0; SetupDiEnumDeviceInterfaces(set, NULL, &twd_guid, i, &iface); i++) {
                union {
                    SP_DEVICE_INTERFACE_DETAIL_DATA_W detail;
                    BYTE raw[sizeof(SP_DEVICE_INTERFACE_DETAIL_DATA_W) + 1024];
                } u;
                struct twd_info info;
                HANDLE device;

                u.detail.cbSize = sizeof(u.detail);
                if (!SetupDiGetDeviceInterfaceDetailW(set, &iface, &u.detail, sizeof(u), NULL, NULL))
                    continue;
                device = CreateFileW(u.detail.DevicePath, GENERIC_READ | GENERIC_WRITE,
                                     FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_EXISTING, 0, NULL);
                if (device == INVALID_HANDLE_VALUE) {
                    printf("TWD info open-failed path=%ls error=%lu\n", u.detail.DevicePath, GetLastError());
                    continue;
                }
                seen++;
                if (get_info(device, &info)) {
                    printf("TWD info path=%ls\n", u.detail.DevicePath);
                    print_info(&info);
                    if (info.vendor == vid && info.product == pid) {
                        *found = info;
                        SetupDiDestroyDeviceInfoList(set);
                        return device;
                    }
                }
                CloseHandle(device);
            }
            SetupDiDestroyDeviceInfoList(set);
        }
        if (now_seconds() > deadline) {
            printf("TWD info no-device vid=%04x pid=%04x twdusb-devices-seen=%lu\n", vid, pid, seen);
            return INVALID_HANDLE_VALUE;
        }
        if (!printed_none) {
            printf("TWD info waiting for vid=%04x pid=%04x\n", vid, pid);
            printed_none = 1;
        }
        Sleep(500);
    }
}

// One transfer through TWD_IOCTL_TRANSFER. Returns FALSE when the IOCTL
// itself failed; the transfer's own status is in *result.
static BOOL transfer(HANDLE device, ULONG kind, ULONG direction, UCHAR endpoint, UCHAR request, USHORT value,
                     USHORT index, const void *out_data, void *in_data, ULONG length, ULONG timeout_ms,
                     struct twd_result *result, double *seconds)
{
    BOOL in = kind == TWD_KIND_PIPE ? (endpoint & 0x80) != 0 : direction == TWD_DIR_IN;
    ULONG in_size = sizeof(struct twd_transfer) + (in ? 0 : length);
    ULONG out_size = sizeof(struct twd_result) + (in ? length : 0);
    BYTE *in_buffer = calloc(1, in_size), *out_buffer = calloc(1, out_size);
    struct twd_transfer *req = (struct twd_transfer *)in_buffer;
    DWORD returned = 0;
    double start;
    BOOL ok;

    req->kind = kind;
    req->direction = direction;
    req->endpoint = endpoint;
    req->request = request;
    req->value = value;
    req->index = index;
    req->length = length;
    req->timeout_ms = timeout_ms;
    if (!in && length)
        memcpy(in_buffer + sizeof(*req), out_data, length);
    start = now_seconds();
    ok = DeviceIoControl(device, TWD_IOCTL_TRANSFER, in_buffer, in_size, out_buffer, out_size, &returned, NULL);
    if (seconds)
        *seconds = now_seconds() - start;
    memset(result, 0, sizeof(*result));
    if (ok && returned >= sizeof(*result)) {
        memcpy(result, out_buffer, sizeof(*result));
        if (in && in_data && result->length)
            memcpy(in_data, out_buffer + sizeof(*result), result->length);
    } else {
        result->status = (LONG)0xC0000001;
        printf("TWD info ioctl-failed error=%lu returned=%lu\n", GetLastError(), returned);
    }
    free(in_buffer);
    free(out_buffer);
    return ok;
}

static const char *cancel_name(ULONG path)
{
    switch (path) {
    case TWD_CANCEL_NONE: return "none";
    case TWD_CANCEL_IOCANCELIRP: return "IoCancelIrp";
    case TWD_CANCEL_ABORT_PIPE: return "URB_FUNCTION_ABORT_PIPE";
    default: return "stuck";
    }
}

static UCHAR find_pipe(const struct twd_info *info, UCHAR type, BOOL in)
{
    int i;
    for (i = 0; i < info->num_pipes && i < TWD_MAX_PIPES; i++)
        if (info->pipes[i].type == type && ((info->pipes[i].endpoint & 0x80) != 0) == in)
            return info->pipes[i].endpoint;
    return 0;
}

static void fill_random(BYTE *buffer, ULONG length, ULONG seed)
{
    ULONG i;
    for (i = 0; i < length; i++) {
        seed = seed * 1103515245u + 12345u;
        buffer[i] = (BYTE)(seed >> 16);
    }
}

static void test_loopback(HANDLE device, const struct twd_info *info)
{
    static const ULONG sizes[] = {64, 500, 4096};
    UCHAR ep_out = find_pipe(info, 2, FALSE), ep_in = find_pipe(info, 2, TRUE);
    BYTE out[4096], in[4096];
    struct twd_result r;
    double seconds, total = 0;
    int i, iterations = 200, good = 0;

    check("pipes", ep_out && ep_in, "bulk-out=%02x bulk-in=%02x", ep_out, ep_in);
    if (!ep_out || !ep_in)
        return;

    for (i = 0; i < 3; i++) {
        ULONG size = sizes[i];
        char name[64];
        BOOL ok;
        fill_random(out, size, 1000 + size);
        memset(in, 0, sizeof(in));
        transfer(device, TWD_KIND_PIPE, 0, ep_out, 0, 0, 0, out, NULL, size, 5000, &r, NULL);
        ok = r.status == 0 && r.length == size;
        snprintf(name, sizeof(name), "bulk-out-%lu", size);
        check(name, ok, "status=%#lx usbd=%#lx length=%lu cancel=%s", (unsigned long)r.status,
              (unsigned long)r.usbd_status, r.length, cancel_name(r.cancel_path));
        transfer(device, TWD_KIND_PIPE, 0, ep_in, 0, 0, 0, NULL, in, size, 5000, &r, NULL);
        ok = r.status == 0 && r.length == size && !memcmp(in, out, size);
        snprintf(name, sizeof(name), "bulk-echo-%lu", size);
        check(name, ok, "status=%#lx usbd=%#lx length=%lu same-bytes=%d cancel=%s", (unsigned long)r.status,
              (unsigned long)r.usbd_status, r.length, r.length == size && !memcmp(in, out, size),
              cancel_name(r.cancel_path));
    }

    for (i = 0; i < iterations; i++) {
        fill_random(out, 4096, (ULONG)i);
        transfer(device, TWD_KIND_PIPE, 0, ep_out, 0, 0, 0, out, NULL, 4096, 5000, &r, &seconds);
        total += seconds;
        if (r.status)
            break;
        transfer(device, TWD_KIND_PIPE, 0, ep_in, 0, 0, 0, NULL, in, 4096, 5000, &r, &seconds);
        total += seconds;
        if (r.status || r.length != 4096 || memcmp(in, out, 4096))
            break;
        good++;
    }
    check("bulk-echo-repeated", good == iterations, "%d of %d round trips of 4096 bytes intact", good, iterations);
    if (good && total > 0)
        printf("TWD metric loopback_round_trip_ms %.3f\nTWD metric loopback_throughput_kib_s %.1f\n",
               1000.0 * total / good, (2.0 * 4096 * good / 1024.0) / total);

    // Nothing is queued on the IN side now: a read has to time out, and the
    // driver has to end it. Which way works says what Wine's bus driver
    // supports.
    transfer(device, TWD_KIND_PIPE, 0, ep_in, 0, 0, 0, NULL, in, 64, 1000, &r, &seconds);
    printf("TWD metric timeout_read_seconds %.2f\n", seconds);
    check("timeout-ends", r.cancel_path == TWD_CANCEL_IOCANCELIRP || r.cancel_path == TWD_CANCEL_ABORT_PIPE,
          "status=%#lx usbd=%#lx ended-by=%s", (unsigned long)r.status, (unsigned long)r.usbd_status,
          cancel_name(r.cancel_path));
    printf("TWD info cancel-path %s\n", cancel_name(r.cancel_path));

    fill_random(out, 64, 77);
    transfer(device, TWD_KIND_PIPE, 0, ep_out, 0, 0, 0, out, NULL, 64, 5000, &r, NULL);
    transfer(device, TWD_KIND_PIPE, 0, ep_in, 0, 0, 0, NULL, in, 64, 5000, &r, NULL);
    check("bulk-echo-after-timeout", r.status == 0 && r.length == 64 && !memcmp(in, out, 64),
          "status=%#lx usbd=%#lx length=%lu", (unsigned long)r.status, (unsigned long)r.usbd_status, r.length);
}

static void test_sourcesink(HANDLE device, const struct twd_info *info)
{
    UCHAR ep_out = find_pipe(info, 2, FALSE), ep_in = find_pipe(info, 2, TRUE);
    BYTE out[4096], in[4096];
    USHORT max_packet = 0;
    struct twd_result r;
    ULONG i;
    int ok;

    for (i = 0; i < info->num_pipes; i++)
        if (info->pipes[i].endpoint == ep_in)
            max_packet = info->pipes[i].max_packet;
    check("pipes", ep_out && ep_in, "bulk-out=%02x bulk-in=%02x max-packet=%u", ep_out, ep_in, max_packet);

    // f_sourcesink: 0x5b (vendor, device, OUT) fills ep0's buffer, 0x5c (IN)
    // returns it.
    fill_random(out, 64, 4242);
    transfer(device, TWD_KIND_VENDOR, TWD_DIR_OUT, 0, 0x5b, 0, 0, out, NULL, 64, 5000, &r, NULL);
    check("vendor-control-out", r.status == 0, "request=0x5b status=%#lx usbd=%#lx length=%lu",
          (unsigned long)r.status, (unsigned long)r.usbd_status, r.length);
    memset(in, 0, 64);
    transfer(device, TWD_KIND_VENDOR, TWD_DIR_IN, 0, 0x5c, 0, 0, NULL, in, 64, 5000, &r, NULL);
    ok = r.status == 0 && r.length == 64 && !memcmp(in, out, 64);
    check("vendor-control-in-roundtrip", ok, "request=0x5c status=%#lx usbd=%#lx length=%lu same-bytes=%d",
          (unsigned long)r.status, (unsigned long)r.usbd_status, r.length, r.length == 64 && !memcmp(in, out, 64));

    if (!ep_out || !ep_in || !max_packet)
        return;
    // pattern=1: byte i of a transfer is (i % wMaxPacketSize) % 63.
    transfer(device, TWD_KIND_PIPE, 0, ep_in, 0, 0, 0, NULL, in, 4096, 5000, &r, NULL);
    ok = r.status == 0 && r.length == 4096;
    for (i = 0; ok && i < r.length; i++)
        if (in[i] != (BYTE)((i % max_packet) % 63))
            ok = 0;
    check("bulk-in-pattern", ok, "status=%#lx usbd=%#lx length=%lu", (unsigned long)r.status,
          (unsigned long)r.usbd_status, r.length);
    for (i = 0; i < 4096; i++)
        out[i] = (BYTE)((i % max_packet) % 63);
    transfer(device, TWD_KIND_PIPE, 0, ep_out, 0, 0, 0, out, NULL, 4096, 5000, &r, NULL);
    check("bulk-out-pattern-accepted", r.status == 0 && r.length == 4096, "status=%#lx usbd=%#lx length=%lu",
          (unsigned long)r.status, (unsigned long)r.usbd_status, r.length);
}

// Echo 64 bytes on the loopback gadget again and again for `seconds`, and say
// when, if ever, it stopped working. Used to see what an application sees
// when its device grant is withdrawn while it runs.
static void test_soak(HANDLE device, const struct twd_info *info, int seconds)
{
    UCHAR ep_out = find_pipe(info, 2, FALSE), ep_in = find_pipe(info, 2, TRUE);
    double start = now_seconds();
    BYTE out[64], in[64];
    struct twd_result r;
    unsigned long rounds = 0;

    while (now_seconds() - start < seconds) {
        fill_random(out, sizeof(out), (ULONG)rounds);
        transfer(device, TWD_KIND_PIPE, 0, ep_out, 0, 0, 0, out, NULL, sizeof(out), 3000, &r, NULL);
        if (r.status == 0)
            transfer(device, TWD_KIND_PIPE, 0, ep_in, 0, 0, 0, NULL, in, sizeof(in), 3000, &r, NULL);
        if (r.status != 0 || r.length != sizeof(in) || memcmp(in, out, sizeof(in))) {
            printf("TWD metric soak_stopped_after_seconds %.2f\n", now_seconds() - start);
            check("soak", 0, "stopped after %lu round trips, %.2f s: status=%#lx usbd=%#lx ended-by=%s", rounds,
                  now_seconds() - start, (unsigned long)r.status, (unsigned long)r.usbd_status,
                  cancel_name(r.cancel_path));
            return;
        }
        rounds++;
    }
    check("soak", 1, "%lu round trips in %d s", rounds, seconds);
}

static void test_interrupt(HANDLE device, const struct twd_info *info)
{
    UCHAR ep_in = find_pipe(info, 3, TRUE);
    BYTE in[64];
    struct twd_result r;

    check("pipes", ep_in != 0, "interrupt-in=%02x", ep_in);
    if (!ep_in)
        return;
    memset(in, 0, sizeof(in));
    transfer(device, TWD_KIND_PIPE, 0, ep_in, 0, 0, 0, NULL, in, 32, 3000, &r, NULL);
    check("interrupt-in", r.status == 0 && r.length > 0 && !memcmp(in, "TURHID", 6),
          "status=%#lx usbd=%#lx length=%lu ended-by=%s first-bytes=%.6s", (unsigned long)r.status,
          (unsigned long)r.usbd_status, r.length, cancel_name(r.cancel_path), r.length >= 6 ? (char *)in : "");
}

int main(int argc, char **argv)
{
    struct twd_info info;
    unsigned vid, pid;
    int wait_seconds;
    HANDLE device;

    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc < 5) {
        printf("usage: twdtest.exe info|loopback|sourcesink|interrupt VID PID WAIT_SECONDS\n");
        return 2;
    }
    vid = strtoul(argv[2], NULL, 16);
    pid = strtoul(argv[3], NULL, 16);
    wait_seconds = atoi(argv[4]);

    device = find_device(vid, pid, wait_seconds, &info);
    if (device == INVALID_HANDLE_VALUE) {
        check("device-found", 0, "no twdusb device %04x:%04x within %d s", vid, pid, wait_seconds);
        return 3;
    }
    check("device-found", 1, "%04x:%04x", vid, pid);
    check("driver-started", info.start_step == TWD_STEP_DONE && info.start_status == 0,
          "start_step=%lu start_status=%#lx", info.start_step, (unsigned long)info.start_status);

    if (!strcmp(argv[1], "loopback"))
        test_loopback(device, &info);
    else if (!strcmp(argv[1], "sourcesink"))
        test_sourcesink(device, &info);
    else if (!strcmp(argv[1], "interrupt"))
        test_interrupt(device, &info);
    else if (!strcmp(argv[1], "soak"))
        test_soak(device, &info, argc > 5 ? atoi(argv[5]) : 20);

    CloseHandle(device);
    printf("TWD done failures=%d\n", failures);
    return failures ? 1 : 0;
}
