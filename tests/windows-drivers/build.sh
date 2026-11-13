#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Builds the Windows side of the windows-drivers experiment with mingw-w64.
# Nothing built is committed.
#
#   tests/windows-drivers/build.sh OUTPUT_DIR
#
# Drivers (PE images, native subsystem, x86-64), loaded only by Wine's
# winedevice.exe, a Linux user-space process:
#   twdusb.sys      the TUR WDM USB function driver (driver/twdusb.c)
#   kmdfprobe.sys   binds to KMDF through WDFLDR.SYS (driver/kmdfprobe.c)
# Programs (x86-64 console):
#   twdinst.exe     installs driver packages (app/twdinst.c)
#   twdtest.exe     talks to twdusb.sys (app/twdtest.c)
#   hidtest.exe     talks to a HID device through hid.dll (app/hidtest.c)
#   winusbtest.exe  calls WinUsb_Initialize, Wine's own winusb.dll (app/winusbtest.c)
# and the INF files beside the drivers. OUTPUT_DIR/imports.txt lists what
# each image imports.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
out="${1:?output directory required}"
mkdir -p "${out}"
cc=x86_64-w64-mingw32-gcc
dlltool=x86_64-w64-mingw32-dlltool
objdump=x86_64-w64-mingw32-objdump

common=(-O2 -Wall -Wextra -Wno-unused-parameter -D_WIN32_WINNT=0x0A00 -DWINVER=0x0A00)
# A driver: no C runtime, no start files, the native subsystem, DriverEntry
# as the entry point, imports from ntoskrnl.exe, hal.dll and usbd.sys only.
driver_flags=(-fno-stack-protector -fno-tree-loop-distribute-patterns -DNTDDI_VERSION=0x0A000000
    -shared -nostdlib -nostartfiles
    -Wl,--subsystem,native -Wl,--entry,DriverEntry -Wl,--image-base,0x140000000
    -Wl,--file-alignment,0x1000 -Wl,--section-alignment,0x1000)

echo "== twdusb.sys"
"${cc}" "${common[@]}" "${driver_flags[@]}" -o "${out}/twdusb.sys" "${here}/driver/twdusb.c" \
    -lusbd -lntoskrnl -lhal -lgcc

echo "== kmdfprobe.sys"
"${dlltool}" -d "${here}/driver/wdfldr.def" -l "${out}/libwdfldr.a"
"${cc}" "${common[@]}" "${driver_flags[@]}" -o "${out}/kmdfprobe.sys" "${here}/driver/kmdfprobe.c" \
    -L"${out}" -lwdfldr -lntoskrnl -lgcc

echo "== programs"
"${cc}" "${common[@]}" -municode -o "${out}/twdinst.exe" "${here}/app/twdinst.c" -lsetupapi -lnewdev
"${cc}" "${common[@]}" -o "${out}/twdtest.exe" "${here}/app/twdtest.c" -lsetupapi
"${cc}" "${common[@]}" -o "${out}/hidtest.exe" "${here}/app/hidtest.c" -lsetupapi -lhid
"${cc}" "${common[@]}" -o "${out}/winusbtest.exe" "${here}/app/winusbtest.c" -lsetupapi -lwinusb ||
    echo "winusbtest.exe: not built (the harness records it)"

cp "${here}/driver/twdusb.inf" "${here}/driver/kmdfprobe.inf" "${out}/"

{
    for image in twdusb.sys kmdfprobe.sys twdinst.exe twdtest.exe hidtest.exe winusbtest.exe; do
        echo "== ${image}"
        [ -f "${out}/${image}" ] || { echo "  (not built)"; continue; }
        "${objdump}" -p "${out}/${image}" | grep -E '^(Subsystem|AddressOfEntryPoint|ImageBase)' || true
        "${objdump}" -p "${out}/${image}" | awk '/DLL Name:/{print "  " $0} /^\t[0-9a-f]+ +[0-9]+ +[A-Za-z_]/{print "    " $NF}'
    done
} >"${out}/imports.txt"
cat "${out}/imports.txt"
ls -l "${out}"
