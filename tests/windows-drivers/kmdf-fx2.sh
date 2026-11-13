#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Best-effort: build Microsoft's kmdf_fx2 sample (the OSR USB FX2 KMDF
# driver) with mingw-w64 and the framework headers of Microsoft's
# Windows-Driver-Frameworks, and record how far that gets. Never fails: the
# result is written to OUTPUT_DIR/kmdf-fx2/summary.txt and, if an image was
# linked, OUTPUT_DIR/osrusbfx2.sys and osrusbfx2.inf are left for the
# harness.
#
#   tests/windows-drivers/kmdf-fx2.sh OUTPUT_DIR
#
# Sources, fetched here at pinned commits and never committed to TUR:
#   microsoft/Windows-driver-samples  usb/kmdf_fx2           MS-PL
#   microsoft/Windows-Driver-Frameworks src/publicinc/wdf/kmdf MIT
# Licences: https://github.com/microsoft/Windows-driver-samples/blob/main/LICENSE
# and https://github.com/microsoft/Windows-Driver-Frameworks/blob/main/LICENSE.
# The WDK's own tools (WPP's tracewpp, the message compiler, stampinf) are
# Microsoft binaries and are not used: their generated files are replaced by
# empty stand-ins (shim/), which turns tracing and ETW events off.
set -uo pipefail

SAMPLES_REPO=https://github.com/microsoft/Windows-driver-samples
SAMPLES_COMMIT=d49bb7d72f3651952b2b6e4ad28d14fe2247eb69
WDF_REPO=https://github.com/microsoft/Windows-Driver-Frameworks
WDF_COMMIT=b6191d9543441329154da32f7ab9bdd97228dd3c
KMDF_MINOR=33

here="$(cd "$(dirname "$0")" && pwd)"
out="$(mkdir -p "${1:?output directory required}" && cd "$1" && pwd)"
work="${out}/kmdf-fx2"
rm -rf "${work}"
mkdir -p "${work}/shim" "${work}/obj"
summary="${work}/summary.txt"
log="${work}/build.log"
: >"${summary}"
note() { echo "$*" | tee -a "${summary}"; }

fetch() { # repo commit dir path
    git init -q "$3" &&
        git -C "$3" remote add origin "$1" &&
        git -C "$3" sparse-checkout set --no-cone "$4" &&
        git -C "$3" fetch -q --depth 1 --filter=blob:none origin "$2" &&
        git -C "$3" checkout -q FETCH_HEAD
}
if ! fetch "${SAMPLES_REPO}" "${SAMPLES_COMMIT}" "${work}/samples" "/usb/kmdf_fx2/" >>"${log}" 2>&1 ||
    ! fetch "${WDF_REPO}" "${WDF_COMMIT}" "${work}/wdf" "/src/publicinc/wdf/kmdf/1.${KMDF_MINOR}/" >>"${log}" 2>&1; then
    note "fetch: failed (see build.log)"
    exit 0
fi
note "fetch: Windows-driver-samples ${SAMPLES_COMMIT} usb/kmdf_fx2, Windows-Driver-Frameworks ${WDF_COMMIT} kmdf/1.${KMDF_MINOR}"
sample="${work}/samples/usb/kmdf_fx2"
wdfinc="${work}/wdf/src/publicinc/wdf/kmdf/1.${KMDF_MINOR}"

ddk="$(dirname "$(echo '#include <ddk/wdm.h>' | x86_64-w64-mingw32-gcc -xc -E -H - 2>&1 >/dev/null | grep -m1 'ddk/wdm.h' | sed 's/^\.* //')")"
note "mingw-w64 ddk headers: ${ddk}"

# Stand-ins for the files WDK tools generate.
for tmh in $(grep -ho '"[A-Za-z0-9_]*\.tmh"' "${sample}"/driver/*.c | tr -d '"' | sort -u); do
    : >"${work}/shim/${tmh}"
done
{
    echo "/* Stand-in for the message compiler's output: ETW events off. */"
    echo "#pragma once"
    for name in $(grep -ho 'Event\(Write\|Register\|Unregister\)[A-Za-z0-9_]*' "${sample}"/driver/*.c | sort -u); do
        echo "#define ${name}(...) ((NTSTATUS)0)"
    done
} >"${work}/shim/fx2Events.h"
cat >"${work}/shim/twd-kmdf-shim.h" <<'EOF'
/* Annotations and definitions the WDK's compiler provides and mingw-w64's
   headers do not; all compile to nothing. */
#pragma once
#ifndef _Must_inspect_result_
#define _Must_inspect_result_
#endif
#define WPP_INIT_TRACING(d, r)
#define WPP_CLEANUP(d)
EOF
note "shims: $(ls "${work}/shim" | tr '\n' ' ')"

cflags=(-c -O1 -w -fms-extensions -D_AMD64_ -DAMD64 -D_WIN64 -DNTDDI_VERSION=0x0A000000 -D_WIN32_WINNT=0x0A00
    -DKMDF_VERSION_MAJOR=1 -DKMDF_VERSION_MINOR=${KMDF_MINOR} -DPOOL_NX_OPTIN=0
    -I"${work}/shim" -I"${sample}/inc" -I"${sample}/driver" -I"${wdfinc}" -I"${ddk}"
    -include "${work}/shim/twd-kmdf-shim.h")
ok=1
for src in "${sample}"/driver/*.c; do
    name="$(basename "${src}" .c)"
    if x86_64-w64-mingw32-gcc "${cflags[@]}" -o "${work}/obj/${name}.o" "${src}" >"${work}/${name}.log" 2>&1; then
        note "compile ${name}.c: ok"
    else
        ok=0
        note "compile ${name}.c: FAILED, $(grep -c 'error:' "${work}/${name}.log") errors; first:"
        grep -m 8 'error:' "${work}/${name}.log" | sed "s|${work}/||g; s/^/    /" | tee -a "${summary}"
    fi
done
if [ "${ok}" = 1 ]; then
    x86_64-w64-mingw32-gcc "${cflags[@]}" -o "${work}/obj/kmdfstub.o" "${here}/driver/kmdfstub.c" >>"${log}" 2>&1
    x86_64-w64-mingw32-dlltool -d "${here}/driver/wdfldr.def" -l "${work}/libwdfldr.a"
    if x86_64-w64-mingw32-gcc -shared -nostdlib -nostartfiles -Wl,--subsystem,native -Wl,--entry,FxDriverEntry \
        -Wl,--image-base,0x140000000 -o "${out}/osrusbfx2.sys" "${work}"/obj/*.o \
        -L"${work}" -lwdfldr -lusbd -lntoskrnl -lhal -lwmilib -lgcc >"${work}/link.log" 2>&1; then
        note "link osrusbfx2.sys: ok"
        x86_64-w64-mingw32-objdump -p "${out}/osrusbfx2.sys" | awk '/DLL Name:/{print "  imports " $3}' | tee -a "${summary}"
        sed -e 's/kmdfprobe/osrusbfx2/g' -e 's/KMDF probe/kmdf_fx2 sample/' -e 's/binding probe/kmdf_fx2 sample/' \
            "${here}/driver/kmdfprobe.inf" >"${out}/osrusbfx2.inf"
    else
        note "link osrusbfx2.sys: FAILED; first:"
        grep -m 12 -E 'undefined reference|error' "${work}/link.log" | sed 's/^/    /' | tee -a "${summary}"
    fi
fi
exit 0
