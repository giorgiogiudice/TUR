// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// SEALED (driver-synthesis experiment): gadget.exe, the Windows
// command-line program of the invented desk device. It talks to qfdesk.sys
// through its device interface (SetupDiGetClassDevs, CreateFile,
// DeviceIoControl) and offers every function of the device. Its commands
// and output are the command-line contract the blind engineer's Linux
// driver must follow (handoff/TASK.md), so that the captures' timelines
// show exactly what a correct Linux driver prints.
//
// Each run is one session: connect (the handshake), one command, disconnect.
//
//   gadget.exe COMMAND [ARGS]       (see usage())
//
// Exit status: 0 done, 1 device error, 2 usage, 3 no device, 4 empty note slot.
#include <windows.h>
#include <setupapi.h>
#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "qfdesk.h"

static const GUID qfdesk_guid = QFDESK_INTERFACE_GUID_INIT;
static HANDLE device = INVALID_HANDLE_VALUE;

static void fail(const char *fmt, ...)
{
    va_list args;
    fprintf(stderr, "error: ");
    va_start(args, fmt);
    vfprintf(stderr, fmt, args);
    va_end(args);
    fprintf(stderr, "\n");
    exit(1);
}

static int usage(void)
{
    fprintf(stderr,
            "usage: gadget COMMAND [ARGS]\n"
            "  info | read-temperature | read-humidity | read-light | read-sensors\n"
            "  get-mode | set-mode day|night\n"
            "  set-brightness 0-100 | get-brightness | set-night-level 0-100 | get-night-level\n"
            "  set-alert CELSIUS|off | get-alert\n"
            "  show-image FILE | clear-display\n"
            "  list-notes | write-note SLOT TEXT | read-note SLOT | delete-note SLOT | clear-notes\n"
            "  wait-event SECONDS\n");
    return 2;
}

static HANDLE find_device(int wait_seconds)
{
    DWORD deadline = GetTickCount() + wait_seconds * 1000;

    for (;;) {
        HDEVINFO set = SetupDiGetClassDevsW(&qfdesk_guid, NULL, NULL, DIGCF_DEVICEINTERFACE | DIGCF_PRESENT);
        SP_DEVICE_INTERFACE_DATA iface = {sizeof(iface)};

        if (set != INVALID_HANDLE_VALUE) {
            if (SetupDiEnumDeviceInterfaces(set, NULL, &qfdesk_guid, 0, &iface)) {
                union {
                    SP_DEVICE_INTERFACE_DETAIL_DATA_W detail;
                    BYTE raw[sizeof(SP_DEVICE_INTERFACE_DETAIL_DATA_W) + 1024];
                } u;
                HANDLE h;

                u.detail.cbSize = sizeof(u.detail);
                if (SetupDiGetDeviceInterfaceDetailW(set, &iface, &u.detail, sizeof(u), NULL, NULL)) {
                    h = CreateFileW(u.detail.DevicePath, GENERIC_READ | GENERIC_WRITE,
                                    FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_EXISTING, 0, NULL);
                    if (h != INVALID_HANDLE_VALUE) {
                        SetupDiDestroyDeviceInfoList(set);
                        return h;
                    }
                }
            }
            SetupDiDestroyDeviceInfoList(set);
        }
        if ((LONG) (GetTickCount() - deadline) > 0)
            return INVALID_HANDLE_VALUE;
        Sleep(250);
    }
}

static void dev_ioctl(DWORD code, void *in, DWORD in_len, void *out, DWORD out_len, DWORD *returned)
{
    DWORD got = 0;

    if (!DeviceIoControl(device, code, in, in_len, out, out_len, &got, NULL))
        fail("the device did not answer (error %lu)", GetLastError());
    if (returned)
        *returned = got;
}

// One command; the device's status must be one of the accepted ones.
static unsigned char transact(unsigned char cmd, const void *payload, unsigned char length,
                              struct qfdesk_response *rsp, int accept_status)
{
    struct qfdesk_command c;
    struct qfdesk_response local;

    if (!rsp)
        rsp = &local;
    memset(&c, 0, sizeof(c));
    c.cmd = cmd;
    c.length = length;
    if (length)
        memcpy(c.payload, payload, length);
    dev_ioctl(QFDESK_IOCTL_TRANSACT, &c, sizeof(c), rsp, sizeof(*rsp), NULL);
    if (rsp->status != 0x00 && rsp->status != accept_status)
        fail("the device refused the request (code %02x)", rsp->status);
    return rsp->status;
}

static int get_mode(void)
{
    struct qfdesk_response r;

    transact(0x11, NULL, 0, &r, -1);
    return r.length ? r.payload[0] : -1;
}

static void set_mode(int mode)
{
    unsigned char m = (unsigned char) mode;

    transact(0x10, &m, 1, NULL, -1);
}

// A setting that belongs to one mode: switch to it, do it, switch back.
static void in_mode(int mode, unsigned char cmd, const void *payload, unsigned char length,
                    struct qfdesk_response *rsp)
{
    int was = get_mode();

    if (was != mode)
        set_mode(mode);
    transact(cmd, payload, length, rsp, -1);
    if (was != mode && was >= 0)
        set_mode(was);
}

static int parse_int(const char *s, int lo, int hi)
{
    char *end;
    long v = strtol(s, &end, 10);

    if (!*s || *end || v < lo || v > hi) {
        fprintf(stderr, "error: '%s' is not a number from %d to %d\n", s, lo, hi);
        exit(2);
    }
    return (int) v;
}

static void read_sensors(int *temperature, unsigned *humidity, unsigned long *light)
{
    struct qfdesk_response r;

    transact(0x30, NULL, 0, &r, -1);
    if (r.length < 8)
        fail("short sensor reading");
    *temperature = (short) ((r.payload[0] << 8) | r.payload[1]);
    *humidity = r.payload[2] | (r.payload[3] << 8);
    *light = (unsigned long) r.payload[4] | ((unsigned long) r.payload[5] << 8) |
             ((unsigned long) r.payload[6] << 16) | ((unsigned long) r.payload[7] << 24);
}

static int hexval(int ch)
{
    if (ch == '.')
        return 0;
    if (ch >= '0' && ch <= '9')
        return ch - '0';
    if (ch >= 'a' && ch <= 'f')
        return ch - 'a' + 10;
    if (ch >= 'A' && ch <= 'F')
        return ch - 'A' + 10;
    return -1;
}

static void show_image(const char *path)
{
    unsigned char pix[8][16], packed[64], chunk[33];
    char line[256];
    FILE *f = fopen(path, "r");
    int r = 0, c, k;

    if (!f) {
        fprintf(stderr, "error: cannot open %s\n", path);
        exit(2);
    }
    while (r < 8 && fgets(line, sizeof(line), f)) {
        size_t n = strcspn(line, "\r\n");
        if (n == 0)
            continue;
        if (n != 16) {
            fprintf(stderr, "error: image row %d is not 16 pixels\n", r + 1);
            exit(2);
        }
        for (c = 0; c < 16; c++) {
            int v = hexval(line[c]);
            if (v < 0) {
                fprintf(stderr, "error: bad pixel '%c'\n", line[c]);
                exit(2);
            }
            pix[r][c] = (unsigned char) v;
        }
        r++;
    }
    fclose(f);
    if (r != 8) {
        fprintf(stderr, "error: an image has 8 rows\n");
        exit(2);
    }
    for (c = 0; c < 16; c++)
        for (k = 0; k < 4; k++)
            packed[4 * c + k] = (unsigned char) ((pix[2 * k][c] << 4) | pix[2 * k + 1][c]);
    for (k = 0; k < 2; k++) {
        chunk[0] = (unsigned char) (32 * k);
        memcpy(chunk + 1, packed + 32 * k, 32);
        transact(0x40, chunk, 33, NULL, -1);
    }
    transact(0x41, NULL, 0, NULL, -1);
    printf("image shown\n");
}

static void write_note(int slot, const char *text)
{
    size_t length = strlen(text), done;
    unsigned char begin[2], chunk[41];
    unsigned char index = 0;

    if (length < 1 || length > 120) {
        fprintf(stderr, "error: a note has 1 to 120 bytes\n");
        exit(2);
    }
    begin[0] = (unsigned char) slot;
    begin[1] = (unsigned char) length;
    transact(0x51, begin, 2, NULL, -1);
    for (done = 0; done < length; done += 40) {
        size_t n = length - done < 40 ? length - done : 40;
        chunk[0] = index++;
        memcpy(chunk + 1, text + done, n);
        transact(0x52, chunk, (unsigned char) (n + 1), NULL, -1);
    }
    transact(0x53, NULL, 0, NULL, -1);
    printf("note %d stored\n", slot);
}

static int wait_events(int seconds)
{
    DWORD end = GetTickCount() + (DWORD) seconds * 1000;
    unsigned char ev[8];
    DWORD got;

    fprintf(stderr, "listening for %d s\n", seconds);
    fflush(stderr);
    while ((LONG) (GetTickCount() - end) < 0) {
        dev_ioctl(QFDESK_IOCTL_GET_EVENT, NULL, 0, ev, sizeof(ev), &got);
        if (got < 8) {
            Sleep(40);
            continue;
        }
        if (ev[0] == 0x01)
            printf("button %u %.2f\n", ev[2], (ev[3] | (ev[4] << 8)) / 100.0);
        else if (ev[0] == 0x02)
            printf("alert %.2f\n", (short) ((ev[2] << 8) | ev[3]) / 100.0);
        fflush(stdout);
    }
    return 0;
}

static int run(int argc, char **argv)
{
    const char *cmd = argv[1];
    struct qfdesk_response r;
    int temperature, i;
    unsigned humidity;
    unsigned long light;

    if (!strcmp(cmd, "info")) {
        transact(0x01, NULL, 0, &r, -1);
        if (r.length < 10)
            fail("short answer");
        printf("firmware %u.%u.%u\n", r.payload[0], r.payload[1], r.payload[2]);
        printf("uptime %lu\n", (unsigned long) r.payload[6] | ((unsigned long) r.payload[7] << 8) |
                                   ((unsigned long) r.payload[8] << 16) | ((unsigned long) r.payload[9] << 24));
    }
    else if (!strcmp(cmd, "read-temperature") || !strcmp(cmd, "read-humidity") || !strcmp(cmd, "read-light") ||
             !strcmp(cmd, "read-sensors")) {
        read_sensors(&temperature, &humidity, &light);
        if (!strcmp(cmd, "read-temperature"))
            printf("%.2f\n", temperature / 100.0);
        else if (!strcmp(cmd, "read-humidity"))
            printf("%.1f\n", humidity / 10.0);
        else if (!strcmp(cmd, "read-light"))
            printf("%lu\n", light);
        else
            printf("temperature %.2f\nhumidity %.1f\nlight %lu\n", temperature / 100.0, humidity / 10.0, light);
    }
    else if (!strcmp(cmd, "get-mode")) {
        int m = get_mode();
        printf("%s\n", m == 0 ? "day" : m == 1 ? "night" : m == 2 ? "setup" : "unknown");
    }
    else if (!strcmp(cmd, "set-mode") && argc == 3) {
        if (strcmp(argv[2], "day") && strcmp(argv[2], "night"))
            return usage();
        set_mode(!strcmp(argv[2], "night"));
        printf("mode %s\n", argv[2]);
    }
    else if ((!strcmp(cmd, "set-brightness") || !strcmp(cmd, "set-night-level")) && argc == 3) {
        unsigned char v = (unsigned char) parse_int(argv[2], 0, 100);
        int night = !strcmp(cmd, "set-night-level");
        in_mode(night, 0x21, &v, 1, NULL);
        printf("%s %u\n", night ? "night-level" : "brightness", v);
    }
    else if (!strcmp(cmd, "get-brightness") || !strcmp(cmd, "get-night-level")) {
        in_mode(!strcmp(cmd, "get-night-level"), 0x22, NULL, 0, &r);
        printf("%u\n", r.length ? r.payload[0] : 0);
    }
    else if (!strcmp(cmd, "set-alert") && argc == 3) {
        short v;
        unsigned char p[2];
        if (!strcmp(argv[2], "off")) {
            v = 0x7FFF;
        }
        else {
            char *end;
            double t = strtod(argv[2], &end);
            if (*end || t < -40.0 || t > 125.0) {
                fprintf(stderr, "error: an alert is -40.0 to 125.0 or off\n");
                return 2;
            }
            v = (short) floor(t * 10.0 + 0.5);
        }
        p[0] = (unsigned char) (v & 0xFF);
        p[1] = (unsigned char) ((v >> 8) & 0xFF);
        in_mode(2, 0x21, p, 2, NULL);
        if (v == 0x7FFF)
            printf("alert off\n");
        else
            printf("alert %.1f\n", v / 10.0);
    }
    else if (!strcmp(cmd, "get-alert")) {
        short v;
        in_mode(2, 0x22, NULL, 0, &r);
        v = (short) (r.payload[0] | (r.payload[1] << 8));
        if (v == 0x7FFF)
            printf("off\n");
        else
            printf("%.1f\n", v / 10.0);
    }
    else if (!strcmp(cmd, "show-image") && argc == 3) {
        show_image(argv[2]);
    }
    else if (!strcmp(cmd, "clear-display")) {
        transact(0x42, NULL, 0, NULL, -1);
        printf("display cleared\n");
    }
    else if (!strcmp(cmd, "list-notes")) {
        transact(0x50, NULL, 0, &r, -1);
        for (i = 0; i < 8 && r.length >= 9; i++)
            if (r.payload[0] & (1 << i))
                printf("%d %u\n", i, r.payload[1 + i]);
    }
    else if (!strcmp(cmd, "write-note") && argc == 4) {
        write_note(parse_int(argv[2], 0, 7), argv[3]);
    }
    else if (!strcmp(cmd, "read-note") && argc == 3) {
        unsigned char slot = (unsigned char) parse_int(argv[2], 0, 7);
        if (transact(0x54, &slot, 1, &r, 0x86) == 0x86) {
            printf("empty\n");
            return 4;
        }
        printf("%.*s\n", (int) r.length, (const char *) r.payload);
    }
    else if (!strcmp(cmd, "delete-note") && argc == 3) {
        unsigned char slot = (unsigned char) parse_int(argv[2], 0, 7);
        transact(0x55, &slot, 1, NULL, -1);
        printf("note %u deleted\n", slot);
    }
    else if (!strcmp(cmd, "clear-notes")) {
        // The device wants this one twice: the first only arms it.
        if (transact(0x56, NULL, 0, NULL, 0x01) == 0x01)
            transact(0x56, NULL, 0, NULL, -1);
        printf("notes cleared\n");
    }
    else if (!strcmp(cmd, "wait-event") && argc == 3) {
        return wait_events(parse_int(argv[2], 1, 600));
    }
    else {
        return usage();
    }
    return 0;
}

int main(int argc, char **argv)
{
    struct qfdesk_hello hello;
    DWORD got;
    int rc;

    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc < 2)
        return usage();
    device = find_device(30);
    if (device == INVALID_HANDLE_VALUE) {
        fprintf(stderr, "error: no Glimmerdesk found\n");
        return 3;
    }
    dev_ioctl(QFDESK_IOCTL_CONNECT, NULL, 0, &hello, sizeof(hello), &got);
    rc = run(argc, argv);
    dev_ioctl(QFDESK_IOCTL_DISCONNECT, NULL, 0, NULL, 0, NULL);
    CloseHandle(device);
    return rc;
}
