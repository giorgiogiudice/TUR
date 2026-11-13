#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Builds the KMDF client side of the experiment with the same open toolchain
# as the framework (clang/lld, mingw-w64 headers, the stand-ins of
# third-party/wdf/compat, the patched public KMDF 1.33 headers build-wdf.sh
# left in OUTPUT_DIR/wdf/src):
#
#   osrusbfx2.sys / .inf   Microsoft's kmdf_fx2 sample driver (Windows-driver-
#                          samples usb/kmdf_fx2, MS-PL, fetched at a pinned
#                          commit, never committed) and an INF written here
#   osrusbfx2.exe          the sample's own test program (exe/testapp.c)
#   kmdftest.sys / .inf    the experiment's minimal KMDF driver (kmdftest.c)
#   kmdftest.exe           its test program (kmdftestapp.c)
#
# Each KMDF driver is linked with fxstub.c, the stand-in for the WDK's KMDF
# stub library, and imports WdfVersionBind from WDFLDR.SYS.
#
#   tests/windows-drivers/kmdf/build-fx2.sh OUTPUT_DIR
#
# Output in OUTPUT_DIR/fx2/out; record in OUTPUT_DIR/fx2/summary.txt. Never
# fails: what did not build is the record.
set -uo pipefail

SAMPLES_REPO=https://github.com/microsoft/Windows-driver-samples
SAMPLES_COMMIT=d49bb7d72f3651952b2b6e4ad28d14fe2247eb69
KMDF_MINOR=33

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "${here}/../../.." && pwd)"
compat="${repo}/third-party/wdf/compat"
out="$(cd "${1:?output directory required}" && pwd)"
work="${out}/fx2"
rm -rf "${work}"
mkdir -p "${work}/out" "${work}/obj" "${work}/gen" "${work}/logs"
summary="${work}/summary.txt"
: >"${summary}"
note() { echo "$*" | tee -a "${summary}"; }

pub="${out}/wdf/src/src/publicinc/wdf/kmdf/1.${KMDF_MINOR}"
[ -f "${pub}/wdf.h" ] || { note "KMDF headers: not found (build-wdf.sh did not run)"; exit 0; }
sysroot=/usr/x86_64-w64-mingw32/sys-root/mingw
ddk="${sysroot}/include/ddk"

samples="${work}/samples"
git init -q "${samples}" && git -C "${samples}" remote add origin "${SAMPLES_REPO}" &&
    git -C "${samples}" sparse-checkout set --no-cone /usb/kmdf_fx2/ &&
    git -C "${samples}" fetch -q --depth 1 --filter=blob:none origin "${SAMPLES_COMMIT}" &&
    git -C "${samples}" checkout -q FETCH_HEAD || { note "fetch of the sample: FAILED"; }
sample="${samples}/usb/kmdf_fx2"
[ -d "${sample}/driver" ] && note "sample: ${SAMPLES_REPO} ${SAMPLES_COMMIT} usb/kmdf_fx2 (MS-PL)"

# Generated-file stand-ins for the sample: WPP is off (EVENT_TRACING is not
# defined, so the .tmh files are not included) and the ETW manifest's
# message-compiler header becomes no-op macros.
if [ -d "${sample}/driver" ]; then
    {
        echo "/* Stand-in for the message compiler's output: ETW events off. */"
        echo "#pragma once"
        for name in $(grep -ho 'Event\(Write\|Register\|Unregister\)[A-Za-z0-9_]*' "${sample}"/driver/*.c | sort -u); do
            echo "#define ${name}(...) ((NTSTATUS)0)"
        done
    } >"${work}/gen/fx2Events.h"
fi
python3 -I "${here}/salgen.py" "${work}/gen/tur-sal-generated.h" "${pub}" "${compat}" "${here}" \
    ${sample:+"${sample}"} >"${work}/salgen.log" 2>&1
incdirs=("${compat}" "${work}/gen" "${here}" "${pub}" "${ddk}")
[ -d "${sample}" ] && incdirs=("${compat}" "${work}/gen" "${sample}/inc" "${sample}/driver" "${here}" "${pub}" "${ddk}")
python3 -I "${here}/incmap.py" "${work}/gen" "${here}" "${pub}" ${sample:+"${sample}"} -- "${incdirs[@]}" "${sysroot}/include" \
    >"${work}/incmap.log" 2>&1

cc=(clang --target=x86_64-w64-mingw32)
cflags=(-std=gnu11 -O1 -g0 -fms-extensions -fno-stack-protector -fno-builtin-malloc -mno-red-zone -w
    -D_AMD64_ -DAMD64 -D_WIN64 -D_KERNEL_MODE -DNTDDI_VERSION=0x0A00000C -D_WIN32_WINNT=0x0A00 -DWINVER=0x0A00
    -DKMDF_VERSION_MAJOR=1 -DKMDF_VERSION_MINOR=${KMDF_MINOR} -DKMDF_MINIMUM_VERSION_REQUIRED=${KMDF_MINOR}
    -DPOOL_NX_OPTIN=1 -include "${compat}/tur-wdf-prelude.h")
for d in "${incdirs[@]}"; do cflags+=(-I"${d}"); done

echo "import libraries: $(bash "${here}/implibs.sh" "${work}/implib" "${out}/wine-src")" >>"${summary}"
libgcc="$(x86_64-w64-mingw32-gcc -print-libgcc-file-name 2>/dev/null)"
link_driver() { # name objects...
    local name="$1"
    shift
    if clang --target=x86_64-w64-mingw32 -fuse-ld=lld -nostdlib -nostartfiles -shared \
        -Wl,--subsystem,native -Wl,--entry,FxDriverEntry -Wl,--image-base,0x140000000 \
        -Wl,--file-alignment,0x1000 -Wl,--section-alignment,0x1000 \
        -o "${work}/out/${name}.sys" "$@" -L"${work}/implib" -lwdfldr -lusbd -lntoskrnl -lhal \
        ${libgcc:+"${libgcc}"} >"${work}/logs/${name}-link.log" 2>&1; then
        note "link ${name}.sys: ok; imports: $(llvm-objdump -p "${work}/out/${name}.sys" | awk '/DLL Name:/{printf "%s ", $3}')"
    else
        note "link ${name}.sys: FAILED; first:"
        grep -E 'undefined symbol|error' "${work}/logs/${name}-link.log" | head -n 30 | sed 's/^/    /' | tee -a "${summary}"
    fi
}
compile() { # source object log -> 0/1
    if "${cc[@]}" "${cflags[@]}" -c -o "$2" "$1" >"$3" 2>&1; then
        return 0
    fi
    note "compile $(basename "$1"): FAILED, $(grep -c 'error:' "$3") errors; first:"
    grep -m 12 'error:' "$3" | sed "s|${work}/||g; s|${out}/||g; s/^/    /" | tee -a "${summary}"
    return 1
}

compile "${here}/fxstub.c" "${work}/obj/fxstub.o" "${work}/logs/fxstub.log" && note "compile fxstub.c: ok"

# kmdftest
if compile "${here}/kmdftest.c" "${work}/obj/kmdftest.o" "${work}/logs/kmdftest.log"; then
    note "compile kmdftest.c: ok"
    link_driver kmdftest "${work}/obj/kmdftest.o" "${work}/obj/fxstub.o"
fi
x86_64-w64-mingw32-gcc -O2 -Wall -o "${work}/out/kmdftest.exe" -I"${here}" "${here}/kmdftestapp.c" -lsetupapi \
    >"${work}/logs/kmdftestapp.log" 2>&1 && note "build kmdftest.exe: ok" ||
    { note "build kmdftest.exe: FAILED"; head -n 20 "${work}/logs/kmdftestapp.log" | tee -a "${summary}"; }
sed -e 's/@NAME@/kmdftest/g' -e 's/@HWID@/USB\\VID_1209\&PID_0001/' \
    -e 's/@DESC@/TUR test USB device (loopback gadget), minimal KMDF driver/' \
    "${here}/kmdf-driver.inf.in" >"${work}/out/kmdftest.inf"

# kmdf_fx2
if [ -d "${sample}/driver" ]; then
    ok=1
    objs=()
    for src in "${sample}"/driver/*.c; do
        name="$(basename "${src}" .c)"
        if compile "${src}" "${work}/obj/fx2-${name}.o" "${work}/logs/fx2-${name}.log"; then
            note "compile kmdf_fx2 ${name}.c: ok"
            objs+=("${work}/obj/fx2-${name}.o")
        else
            ok=0
        fi
    done
    [ "${ok}" = 1 ] && link_driver osrusbfx2 "${objs[@]}" "${work}/obj/fxstub.o"
    sed -e 's/@NAME@/osrusbfx2/g' -e 's/@HWID@/USB\\VID_0547\&PID_1002/' \
        -e 's/@DESC@/OSR USB-FX2 (emulated), Microsoft kmdf_fx2 sample driver/' \
        "${here}/kmdf-driver.inf.in" >"${work}/out/osrusbfx2.inf"
    # The program's #include names, resolved case-insensitively as above.
    mkdir -p "${work}/gen-exe"
    python3 -I "${here}/incmap.py" "${work}/gen-exe" "${sample}/exe" -- "${sample}/inc" "${sysroot}/include" \
        >"${work}/incmap-exe.log" 2>&1
    if x86_64-w64-mingw32-gcc -O2 -w -o "${work}/out/osrusbfx2.exe" -I"${work}/gen-exe" -I"${sample}/inc" \
        "${sample}/exe/testapp.c" "${sample}/exe/dump.c" -lsetupapi -lcfgmgr32 >"${work}/logs/testapp.log" 2>&1; then
        note "build osrusbfx2.exe (the sample's testapp.c): ok"
    else
        note "build osrusbfx2.exe (the sample's testapp.c): FAILED; first:"
        grep -m 12 'error' "${work}/logs/testapp.log" | sed 's/^/    /' | tee -a "${summary}"
    fi
fi
ls -l "${work}/out" | tee -a "${summary}"
exit 0
