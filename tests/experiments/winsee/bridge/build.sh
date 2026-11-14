#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Builds the Windows side of the winsee experiment with mingw-w64
# (docs/reports/exp-winsee-1.md). Nothing built is committed.
#
#   tests/experiments/winsee/bridge/build.sh OUTPUT_DIR
#
#   tur-winbridge.exe   the bridge (tw_core.c, tw_msaa.c, tw_uia.c, tw_com.c)
#   tur-winhook.dll     reads a window's menu inside the application (tw_hook.c)
#   tw-controls.exe     a window with one of each standard control (../apps)
#   tw-attack.exe       a window that paints one label over another control
#
# A part of the bridge that does not compile against the headers at hand is
# built again as a stub that says so (-DTW_STUB), so that one failure does
# not hide what the rest measures. OUTPUT_DIR/build.json records which.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
out="${1:?output directory required}"
mkdir -p "${out}/obj" "${out}/logs"
cc=x86_64-w64-mingw32-gcc
common=(-O2 -Wall -Wextra -Wno-unused-parameter -Wno-unused-function -D_WIN32_WINNT=0x0A00 -DWINVER=0x0A00)

declare -A built
status=0

part() { # name: compile bridge/NAME.c, or its stub
    local name="$1"
    if "${cc}" "${common[@]}" -c -o "${out}/obj/${name}.o" "${here}/${name}.c" 2>"${out}/logs/${name}.log"; then
        built[${name}]=built
    else
        echo "::warning::${name}.c did not compile; building its stub"
        head -n 60 "${out}/logs/${name}.log"
        if "${cc}" "${common[@]}" -DTW_STUB -c -o "${out}/obj/${name}.o" "${here}/${name}.c" 2>"${out}/logs/${name}-stub.log"; then
            built[${name}]=stub
        else
            built[${name}]=failed
            head -n 60 "${out}/logs/${name}-stub.log"
            status=1
        fi
    fi
    [ -s "${out}/logs/${name}.log" ] && { echo "-- ${name}.c: compiler output"; head -n 40 "${out}/logs/${name}.log"; }
    return 0
}

echo "== tur-winbridge.exe"
if "${cc}" "${common[@]}" -c -o "${out}/obj/tw_core.o" "${here}/tw_core.c" 2>"${out}/logs/tw_core.log"; then
    built[tw_core]=built
else
    built[tw_core]=failed
    status=1
fi
[ -s "${out}/logs/tw_core.log" ] && head -n 120 "${out}/logs/tw_core.log"
part tw_msaa
part tw_uia
part tw_com
if [ "${built[tw_core]}" = built ]; then
    if "${cc}" -o "${out}/tur-winbridge.exe" "${out}/obj/tw_core.o" "${out}/obj/tw_msaa.o" "${out}/obj/tw_uia.o" \
        "${out}/obj/tw_com.o" -loleacc -loleaut32 -lole32 -luuid -ladvapi32 -luser32 -lgdi32 -lkernel32 \
        2>"${out}/logs/link.log"; then
        built[link]=built
    else
        built[link]=failed
        status=1
        head -n 80 "${out}/logs/link.log"
    fi
else
    built[link]=failed
fi

program() { # output source flags...
    local output="$1" source="$2"
    shift 2
    echo "== ${output}"
    if "${cc}" "${common[@]}" -o "${out}/${output}" "${source}" "$@" 2>"${out}/logs/${output}.log"; then
        built[${output}]=built
    else
        built[${output}]=failed
        status=1
    fi
    [ -s "${out}/logs/${output}.log" ] && head -n 80 "${out}/logs/${output}.log"
    return 0
}

program tur-winhook.dll "${here}/tw_hook.c" -shared -luser32 -lkernel32
program tw-controls.exe "${here}/../apps/tw-controls.c" -mwindows -lcomctl32 -lgdi32 -lshell32 -luser32
program tw-attack.exe "${here}/../apps/tw-attack.c" -mwindows -lgdi32 -lshell32 -luser32

{
    echo "{"
    echo "  \"compiler\": \"$("${cc}" --version | head -n 1)\","
    first=1
    for name in "${!built[@]}"; do
        [ "${first}" = 1 ] || echo ","
        first=0
        printf '  "%s": "%s"' "${name}" "${built[${name}]}"
    done
    echo
    echo "}"
} >"${out}/build.json"
cat "${out}/build.json"
ls -l "${out}"/*.exe "${out}"/*.dll 2>/dev/null || true
x86_64-w64-mingw32-objdump -p "${out}/tur-winbridge.exe" 2>/dev/null | grep 'DLL Name' | sort -u >"${out}/imports.txt" || true
cat "${out}/imports.txt" 2>/dev/null || true
exit "${status}"
