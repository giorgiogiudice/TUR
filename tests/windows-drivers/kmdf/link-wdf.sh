#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Links the objects build-wdf.sh compiled into Wdf01000.sys, a native-
# subsystem PE image (a kernel-mode driver image, loaded here only by Wine's
# winedevice.exe in user space).
#
#   tests/windows-drivers/kmdf/link-wdf.sh WORK_DIR OUTPUT_DIR [WINE_TREE]
#
# mingw-w64's ntstrsafe.h defines its functions "extern __inline", which in
# C gives every object that uses one an identical external copy:
# --allow-multiple-definition keeps the first. A first link without it lists
# every duplicate, and those that are not ntstrsafe.h's are reported.
#
# Import libraries come from implibs.sh (mingw-w64's export lists plus
# Wine's .spec files). A symbol neither knows is the framework's own and is
# reported.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
work="${1:?work directory}"
out="${2:?output directory}"
wine_tree="${3:-}"
lib="${work}/implib"
echo "import libraries: $(bash "${here}/implibs.sh" "${lib}" "${wine_tree}")"

libgcc="$(x86_64-w64-mingw32-gcc -print-libgcc-file-name 2>/dev/null)"
clang++ --target=x86_64-w64-mingw32 -fuse-ld=lld -nostdlib -nostartfiles -shared -Wl,--error-limit=0 \
    -Wl,--subsystem,native -Wl,--entry,TurWdfEntry -o "${work}/dup-check.sys" "${work}"/obj/*.o -L"${lib}" \
    -lntoskrnl -lhal -lusbd -lwdfldr ${libgcc:+"${libgcc}"} >"${work}/dup-check.log" 2>&1
sed -n 's/.*duplicate symbol: //p' "${work}/dup-check.log" | sort -u >"${work}/duplicates.txt"
echo "duplicate symbols without --allow-multiple-definition: $(wc -l <"${work}/duplicates.txt"), not from ntstrsafe.h: $(grep -vc '^RtlString' "${work}/duplicates.txt") $(grep -v '^RtlString' "${work}/duplicates.txt" | head -n 20 | tr '\n' ' ')"
clang++ --target=x86_64-w64-mingw32 -fuse-ld=lld -nostdlib -nostartfiles -shared \
    -Wl,--subsystem,native -Wl,--entry,TurWdfEntry -Wl,--image-base,0x140000000 \
    -Wl,--file-alignment,0x1000 -Wl,--section-alignment,0x1000 -Wl,--allow-multiple-definition \
    -o "${out}/Wdf01000.sys" "${work}"/obj/*.o -L"${lib}" -lntoskrnl -lhal -lusbd -lwdfldr \
    ${libgcc:+"${libgcc}"} >"${work}/link.log" 2>&1
rc=$?
if [ "${rc}" -eq 0 ]; then
    echo "link Wdf01000.sys: ok, $(stat -c %s "${out}/Wdf01000.sys") bytes"
    llvm-objdump -p "${out}/Wdf01000.sys" | awk '/DLL Name:/{print "  imports " $3}'
    llvm-objdump -p "${out}/Wdf01000.sys" >"${work}/Wdf01000.objdump.txt" 2>&1
    llvm-objdump -h "${out}/Wdf01000.sys" | sed 's/^/  /'
else
    n=$(grep -c 'undefined symbol' "${work}/link.log")
    echo "link Wdf01000.sys: FAILED (${n} undefined symbols); first:"
    grep -E 'undefined symbol|error' "${work}/link.log" | sed -E 's/^.*undefined symbol: //' | sort -u | head -n 120 |
        sed 's/^/    /'
fi
exit 0
