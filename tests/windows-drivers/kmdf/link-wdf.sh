#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Links the objects build-wdf.sh compiled into Wdf01000.sys, a native-
# subsystem PE image (a kernel-mode driver image, loaded here only by Wine's
# winedevice.exe in user space).
#
#   tests/windows-drivers/kmdf/link-wdf.sh WORK_DIR OUTPUT_DIR [WINE_TREE]
#
# Import libraries: for each kernel module, every name mingw-w64's import
# library lists (what Windows exports) plus every name Wine's .spec lists,
# made into one .def; WDFLDR.SYS's from Wine's wdfldr.sys.spec (the TUR
# patch). A symbol neither knows is the framework's own and is reported.
set -uo pipefail

work="${1:?work directory}"
out="${2:?output directory}"
wine_tree="${3:-}"
sysroot=/usr/x86_64-w64-mingw32/sys-root/mingw
lib="${work}/implib"
mkdir -p "${lib}"
note() { echo "$*"; }

def_for() { # module(Windows name) mingw-lib-stem wine-spec
    local module="$1" stem="$2" spec="$3"
    {
        echo "LIBRARY \"${module}\""
        echo "EXPORTS"
        {
            [ -f "${sysroot}/lib/lib${stem}.a" ] &&
                llvm-nm -g "${sysroot}/lib/lib${stem}.a" 2>/dev/null | sed -n 's/.* __imp_\([A-Za-z_][A-Za-z0-9_@]*\)$/\1/p'
            [ -n "${spec}" ] && [ -f "${spec}" ] &&
                sed -nE 's/^\s*@\s+(stdcall|cdecl|varargs|stub|extern|fastcall)\s+(-\S+\s+)*([A-Za-z_][A-Za-z0-9_]*).*/\3/p' "${spec}"
        } | sort -u
    } >"${lib}/${stem}.def"
    llvm-dlltool -m i386:x86-64 -d "${lib}/${stem}.def" -l "${lib}/lib${stem}.a"
}
def_for ntoskrnl.exe ntoskrnl "${wine_tree:+${wine_tree}/dlls/ntoskrnl.exe/ntoskrnl.exe.spec}"
def_for hal.dll hal "${wine_tree:+${wine_tree}/dlls/hal/hal.spec}"
def_for usbd.sys usbd "${wine_tree:+${wine_tree}/dlls/usbd.sys/usbd.sys.spec}"
def_for WDFLDR.SYS wdfldr "${wine_tree:+${wine_tree}/dlls/wdfldr.sys/wdfldr.sys.spec}"
note "import libraries: $(for d in "${lib}"/*.def; do printf '%s %s; ' "$(basename "${d}" .def)" "$(($(wc -l <"${d}") - 2))"; done)"

libgcc="$(x86_64-w64-mingw32-gcc -print-libgcc-file-name 2>/dev/null)"
clang++ --target=x86_64-w64-mingw32 -fuse-ld=lld -nostdlib -nostartfiles -shared \
    -Wl,--subsystem,native -Wl,--entry,TurWdfEntry -Wl,--image-base,0x140000000 \
    -Wl,--file-alignment,0x1000 -Wl,--section-alignment,0x1000 \
    -o "${out}/Wdf01000.sys" "${work}"/obj/*.o -L"${lib}" -lntoskrnl -lhal -lusbd -lwdfldr \
    ${libgcc:+"${libgcc}"} >"${work}/link.log" 2>&1
rc=$?
if [ "${rc}" -eq 0 ]; then
    note "link Wdf01000.sys: ok, $(stat -c %s "${out}/Wdf01000.sys") bytes"
    llvm-objdump -p "${out}/Wdf01000.sys" | awk '/DLL Name:/{print "  imports " $3}'
    llvm-objdump -p "${out}/Wdf01000.sys" >"${work}/Wdf01000.objdump.txt" 2>&1
    llvm-objdump -h "${out}/Wdf01000.sys" | sed 's/^/  /'
else
    n=$(grep -c 'undefined symbol' "${work}/link.log")
    note "link Wdf01000.sys: FAILED (${n} undefined symbols); first:"
    grep -E 'undefined symbol|error' "${work}/link.log" | sed -E 's/^.*undefined symbol: //' | sort -u | head -n 80 | sed 's/^/    /'
fi
exit 0
