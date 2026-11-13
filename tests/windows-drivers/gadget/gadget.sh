#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Virtual USB devices for the windows-drivers experiment, made from the
# kernel's own USB gadget framework: dummy_hcd is a host controller and a
# device controller joined in software, so a gadget bound to it appears to
# the same machine as a USB device plugged into a USB port. No hardware, no
# VM, and nothing but stock modules of the runner's kernel.
#
#   gadget.sh probe     what the kernel offers; never fails
#   gadget.sh up        load the modules and create the three gadgets
#   gadget.sh down      remove them
#   gadget.sh show      the devices as the host sees them
#
# The gadgets (pid.codes test IDs, https://pid.codes/1209/0001/ is "Test
# PID"; 0002 and 0003 are ours only on this throw-away machine):
#   1209:0001  Loopback   (usb_f_ss_lb): vendor-class interface, bulk OUT
#              data comes back on bulk IN
#   1209:0002  SourceSink (usb_f_ss_lb, pattern=1): vendor-class interface,
#              vendor control requests 0x5b/0x5c write and read back a
#              buffer, bulk IN sends (i % maxpacket) % 63, bulk OUT checks it
#   1209:0003  HID        (usb_f_hid): vendor usage page 0xff00, 32-byte input
#              and output reports; the host's usbhid binds it and makes a
#              /dev/hidrawN; the gadget side is /dev/hidg0
#
# This is for the CI runner only. It runs as root and loads kernel modules:
# the stock dummy_hcd, libcomposite, usb_f_ss_lb and usb_f_hid, no others.
set -euo pipefail

CONFIGFS=/sys/kernel/config
GADGETS="${CONFIGFS}/usb_gadget"
MODULES=(dummy_hcd libcomposite usb_f_ss_lb usb_f_hid)

# HID report descriptor: Usage Page (Vendor 0xFF00), Usage 1, Collection
# (Application), 32 x 8-bit Input (Usage 2), 32 x 8-bit Output (Usage 3).
HID_REPORT_DESC='\x06\x00\xff\x09\x01\xa1\x01\x15\x00\x26\xff\x00\x75\x08\x95\x20\x09\x02\x81\x02\x95\x20\x09\x03\x91\x02\xc0'

probe() {
    echo "kernel: $(uname -r)"
    echo "config file: /boot/config-$(uname -r)"
    grep -hE '^(# )?CONFIG_(USB_DUMMY_HCD|USB_GADGET|USB_LIBCOMPOSITE|USB_CONFIGFS|USB_CONFIGFS_F_LB_SS|USB_CONFIGFS_F_HID|USB_F_SS_LB|USB_F_HID|USB_RAW_GADGET|USBIP_CORE|USBIP_VHCI_HCD|USB_HIDDEV|HIDRAW)[= ]' \
        "/boot/config-$(uname -r)" 2>/dev/null | sed 's/^/  /' || echo "  (no config file)"
    for module in "${MODULES[@]}" raw_gadget vhci_hcd usbip_host; do
        if path="$(modinfo -n "${module}" 2>/dev/null)"; then
            echo "module ${module}: ${path}"
        else
            echo "module ${module}: not available"
        fi
    done
    echo "/dev/kvm: $(ls -l /dev/kvm 2>&1)"
}

write() { printf '%s' "$2" >"$1"; }

make_gadget() {
    local name="$1" pid="$2" product="$3" function="$4" udc="$5"
    local g="${GADGETS}/${name}"
    mkdir "${g}"
    write "${g}/idVendor" 0x1209
    write "${g}/idProduct" "${pid}"
    write "${g}/bcdDevice" 0x0100
    write "${g}/bcdUSB" 0x0200
    mkdir "${g}/strings/0x409"
    write "${g}/strings/0x409/manufacturer" "TUR test"
    write "${g}/strings/0x409/product" "${product}"
    write "${g}/strings/0x409/serialnumber" "TWD-${pid}"
    mkdir "${g}/configs/c.1"
    write "${g}/configs/c.1/MaxPower" 100
    mkdir "${g}/configs/c.1/strings/0x409"
    write "${g}/configs/c.1/strings/0x409/configuration" "${product}"
    mkdir "${g}/functions/${function}"
    case "${function}" in
    SourceSink.*) write "${g}/functions/${function}/pattern" 1 ;;
    hid.*)
        write "${g}/functions/${function}/protocol" 0
        write "${g}/functions/${function}/subclass" 0
        write "${g}/functions/${function}/report_length" 32
        printf "${HID_REPORT_DESC}" >"${g}/functions/${function}/report_desc"
        ;;
    esac
    ln -s "${g}/functions/${function}" "${g}/configs/c.1/"
    write "${g}/UDC" "${udc}"
}

up() {
    modprobe dummy_hcd num=3
    for module in libcomposite usb_f_ss_lb usb_f_hid; do modprobe "${module}"; done
    mountpoint -q "${CONFIGFS}" || mount -t configfs none "${CONFIGFS}"
    local udcs
    mapfile -t udcs < <(ls /sys/class/udc | sort)
    echo "device controllers: ${udcs[*]}"
    if [ "${#udcs[@]}" -lt 3 ]; then
        echo "need three device controllers, have ${#udcs[@]}" >&2
        exit 1
    fi
    make_gadget twd_loopback 0x0001 "TWD loopback" Loopback.0 "${udcs[0]}"
    make_gadget twd_sourcesink 0x0002 "TWD source-sink" SourceSink.0 "${udcs[1]}"
    make_gadget twd_hid 0x0003 "TWD HID" hid.usb0 "${udcs[2]}"
    # Let the host side enumerate and udev make the nodes.
    for _ in $(seq 1 30); do
        if [ "$(grep -l '^1209$' /sys/bus/usb/devices/*/idVendor 2>/dev/null | wc -l)" -ge 3 ] &&
            ls /dev/hidraw* >/dev/null 2>&1 && [ -e /dev/hidg0 ]; then
            break
        fi
        sleep 1
    done
    udevadm settle || true
    show
}

down() {
    local g f
    for g in "${GADGETS}"/twd_*; do
        [ -d "${g}" ] || continue
        write "${g}/UDC" "" 2>/dev/null || true
        for f in "${g}"/configs/c.1/*.*; do [ -L "${f}" ] && rm "${f}"; done
        rmdir "${g}"/configs/c.1/strings/0x409 "${g}"/configs/c.1 2>/dev/null || true
        for f in "${g}"/functions/*; do [ -d "${f}" ] && rmdir "${f}"; done
        rmdir "${g}"/strings/0x409 "${g}" 2>/dev/null || true
    done
}

show() {
    local d
    for d in /sys/bus/usb/devices/*; do
        [ -f "${d}/idVendor" ] || continue
        [ "$(cat "${d}/idVendor")" = 1209 ] || continue
        printf '%s %s:%s bus %s dev %s speed %s node /dev/bus/usb/%03d/%03d product "%s"\n' \
            "$(basename "${d}")" "$(cat "${d}/idVendor")" "$(cat "${d}/idProduct")" \
            "$(cat "${d}/busnum")" "$(cat "${d}/devnum")" "$(cat "${d}/speed")" \
            "$(cat "${d}/busnum")" "$(cat "${d}/devnum")" "$(cat "${d}/product" 2>/dev/null)"
        for i in "${d}"/*:*; do
            [ -d "${i}" ] || continue
            local driver=none
            [ -L "${i}/driver" ] && driver="$(basename "$(readlink "${i}/driver")")"
            printf '  interface %s class %s driver %s\n' "$(basename "${i}")" \
                "$(cat "${i}/bInterfaceClass")" "${driver}"
        done
    done
    ls -l /dev/bus/usb/*/* 2>/dev/null | sed 's/^/  /'
    ls -l /dev/hidraw* /dev/hidg* 2>/dev/null | sed 's/^/  /' || true
}

case "${1:-}" in
probe) probe ;;
up) up ;;
down) down ;;
show) show ;;
*)
    echo "usage: $0 probe|up|down|show" >&2
    exit 2
    ;;
esac
