#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# SEALED (driver-synthesis experiment): builds qfdesk.sys (the KMDF driver
# of the invented device) and gadget.exe (its command-line program) with
# the open toolchain of the windows-drivers experiment: clang/lld, mingw-w64
# headers, TUR's stand-ins in third-party/wdf/compat, Microsoft's public
# KMDF 1.33 headers as patched by tests/windows-drivers/kmdf/build-wdf.sh,
# and TUR's KMDF stub (tests/windows-drivers/kmdf/fxstub.c).
#
#   build.sh OUTPUT_DIR RUNTIME_DIR
#
# RUNTIME_DIR holds what bench/ci-build.sh kept of the KMDF runtime build:
# kmdf-inc/ (the public headers) and wine-tree/ (Wine's .spec files for
# the import libraries). Output: OUTPUT_DIR/{qfdesk.sys,qfdesk.inf,
# gadget.exe}, OUTPUT_DIR/summary.txt. Fails when something does not build.
set -uo pipefail

KMDF_MINOR=33
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "${here}/../../../../.." && pwd)"
kmdf="${repo}/tests/windows-drivers/kmdf"
compat="${repo}/third-party/wdf/compat"
out="$(mkdir -p "${1:?output directory}" && cd "$1" && pwd)"
runtime="$(cd "${2:?runtime directory}" && pwd)"
pub="${runtime}/kmdf-inc"
work="${out}/work"
rm -rf "${work}"
mkdir -p "${work}/gen" "${work}/obj" "${work}/logs"
summary="${out}/summary.txt"
: >"${summary}"
note() { echo "$*" | tee -a "${summary}"; }
failed=0

[ -f "${pub}/wdf.h" ] || { note "KMDF headers not found in ${pub}"; exit 1; }
sysroot=/usr/x86_64-w64-mingw32/sys-root/mingw
ddk="${sysroot}/include/ddk"

python3 -I "${kmdf}/salgen.py" "${work}/gen/tur-sal-generated.h" "${pub}" "${compat}" "${kmdf}" "${here}" \
    >"${work}/logs/salgen.log" 2>&1 || { note "salgen: FAILED"; exit 1; }
incdirs=("${compat}" "${work}/gen" "${here}" "${kmdf}" "${pub}" "${ddk}")
python3 -I "${kmdf}/incmap.py" "${work}/gen" "${here}" "${kmdf}" "${pub}" -- "${incdirs[@]}" "${sysroot}/include" \
    >"${work}/logs/incmap.log" 2>&1 || { note "incmap: FAILED"; exit 1; }

cflags=(-std=gnu11 -O1 -g0 -fms-extensions -fno-stack-protector -fno-builtin-malloc -mno-red-zone -w
    -Wno-incompatible-pointer-types
    -D_AMD64_ -DAMD64 -D_WIN64 -D_KERNEL_MODE -DNTDDI_VERSION=0x0A00000C -D_WIN32_WINNT=0x0A00 -DWINVER=0x0A00
    -DKMDF_VERSION_MAJOR=1 -DKMDF_VERSION_MINOR=${KMDF_MINOR} -DKMDF_MINIMUM_VERSION_REQUIRED=${KMDF_MINOR}
    -DPOOL_NX_OPTIN=1 -include "${compat}/tur-wdf-prelude.h")
for d in "${incdirs[@]}"; do cflags+=(-I"${d}"); done

compile() { # source object
    local log="${work}/logs/$(basename "$1").log"
    if clang --target=x86_64-w64-mingw32 "${cflags[@]}" -c -o "$2" "$1" >"${log}" 2>&1; then
        note "compile $(basename "$1"): ok"
        return 0
    fi
    note "compile $(basename "$1"): FAILED, $(grep -c 'error:' "${log}") errors; first:"
    grep -m 20 'error:' "${log}" | sed "s|${repo}/||g; s/^/    /" | tee -a "${summary}"
    failed=1
    return 1
}

note "import libraries: $(bash "${kmdf}/implibs.sh" "${work}/implib" "${runtime}/wine-tree")"
compile "${kmdf}/fxstub.c" "${work}/obj/fxstub.o"
if compile "${here}/qfdesk.c" "${work}/obj/qfdesk.o"; then
    libgcc="$(x86_64-w64-mingw32-gcc -print-libgcc-file-name 2>/dev/null)"
    if clang --target=x86_64-w64-mingw32 -fuse-ld=lld -nostdlib -nostartfiles -shared \
        -Wl,--subsystem,native -Wl,--entry,FxDriverEntry -Wl,--image-base,0x140000000 \
        -Wl,--file-alignment,0x1000 -Wl,--section-alignment,0x1000 -Wl,--allow-multiple-definition \
        -o "${out}/qfdesk.sys" "${work}/obj/qfdesk.o" "${work}/obj/fxstub.o" -L"${work}/implib" \
        -lwdfldr -lusbd -lntoskrnl -lhal ${libgcc:+"${libgcc}"} >"${work}/logs/link.log" 2>&1; then
        note "link qfdesk.sys: ok, $(stat -c %s "${out}/qfdesk.sys") bytes; imports: $(llvm-objdump -p "${out}/qfdesk.sys" | awk '/DLL Name:/{printf "%s ", $3}')"
    else
        note "link qfdesk.sys: FAILED"
        grep -E 'undefined symbol|error' "${work}/logs/link.log" | head -n 30 | sed 's/^/    /' | tee -a "${summary}"
        failed=1
    fi
fi

if x86_64-w64-mingw32-gcc -O2 -Wall -Wno-unused-parameter -o "${out}/gadget.exe" -I"${here}" "${here}/gadget.c" \
    -lsetupapi >"${work}/logs/gadget.log" 2>&1; then
    note "build gadget.exe: ok, $(stat -c %s "${out}/gadget.exe") bytes"
    grep -i warning "${work}/logs/gadget.log" | head -n 10 | sed 's/^/    /' | tee -a "${summary}"
else
    note "build gadget.exe: FAILED"
    head -n 30 "${work}/logs/gadget.log" | sed 's/^/    /' | tee -a "${summary}"
    failed=1
fi

sed -e 's/@NAME@/qfdesk/g' -e 's/@HWID@/USB\\VID_1209\&PID_000C/' -e 's/@DESC@/Glimmerdesk G2/' \
    "${kmdf}/kmdf-driver.inf.in" >"${out}/qfdesk.inf"
exit "${failed}"
