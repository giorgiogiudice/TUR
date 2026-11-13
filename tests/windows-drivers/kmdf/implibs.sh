#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Import libraries for linking driver images against Windows kernel modules:
# for each module, every name mingw-w64's import library lists (what Windows
# exports) plus every name Wine's .spec lists, made into one .def and an
# import library with llvm-dlltool. WDFLDR.SYS's come from Wine's
# wdfldr.sys.spec (the TUR patch set) alone.
#
#   tests/windows-drivers/kmdf/implibs.sh OUTPUT_DIR [WINE_TREE]
set -uo pipefail

lib="${1:?output directory}"
wine_tree="${2:-}"
sysroot=/usr/x86_64-w64-mingw32/sys-root/mingw
mkdir -p "${lib}"

def_for() { # module(Windows name) lib-stem wine-spec
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
for d in "${lib}"/*.def; do printf '%s %s; ' "$(basename "${d}" .def)" "$(($(wc -l <"${d}") - 2))"; done
echo
