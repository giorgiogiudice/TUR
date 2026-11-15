#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Builds P@x's bridge into Windows applications with mingw-w64 (x86-64):
#
#   services/tur-winbridge/build.sh OUTPUT_DIR
#
#   tur-winbridge.exe   the bridge: the protocol and the Win32 tier
#                       (tw_core.c), under TUR's licence
#   tur-winhook.dll     reads a window's menu inside the application
#                       (hook-mit/tw_hook.c), under the MIT licence: it is
#                       the one part that runs inside other publishers'
#                       programs (the owner's decision of 14 November 2026)
#
# Runs in the image build (image/winbridge/winbridge.sh, stage "winbridge" of
# image/Containerfile) and in CI jobs that test the bridge; never on a
# development machine (AGENTS.md). Nothing built is committed.
#
# Two builds of the same source with the same compiler give the same bytes:
# no time stamp in the PE headers, source paths mapped, symbols stripped (the
# image workflow builds twice and compares).
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
out="${1:?usage: build.sh OUTPUT_DIR}"
mkdir -p "${out}"
cc=x86_64-w64-mingw32-gcc

common="-O2 -Wall -Wextra -Wno-unused-parameter -Wno-unused-function -D_WIN32_WINNT=0x0A00 -DWINVER=0x0A00"
common="${common} -ffile-prefix-map=${here}=tur-winbridge -fno-ident"
link="-s -Wl,--no-insert-timestamp"

echo "== $(${cc} --version | head -n 1)"

echo "== tur-winbridge.exe"
# shellcheck disable=SC2086  # the flags are split on purpose
${cc} ${common} ${link} -o "${out}/tur-winbridge.exe" "${here}/tw_core.c" -luser32 -lkernel32

echo "== tur-winhook.dll (MIT)"
# shellcheck disable=SC2086
${cc} ${common} ${link} -shared -o "${out}/tur-winhook.dll" "${here}/hook-mit/tw_hook.c" -luser32 -lkernel32

# What each file needs of Wine: only DLLs Wine ships may be named.
for file in tur-winbridge.exe tur-winhook.dll; do
    echo "== ${file}: $(wc -c < "${out}/${file}") bytes, imports:"
    x86_64-w64-mingw32-objdump -p "${out}/${file}" | sed -n 's/^\tDLL Name: //p' | LC_ALL=C sort -u | tee "${out}/${file}.imports"
    while read -r dll; do
        case "$(printf '%s' "${dll}" | tr '[:upper:]' '[:lower:]')" in
            kernel32.dll | user32.dll | msvcrt.dll | ucrtbase.dll | api-ms-win-crt-*.dll) ;;
            *)
                echo "${file} imports ${dll}, which the bridge must not need" >&2
                exit 1
                ;;
        esac
    done < "${out}/${file}.imports"
    rm -f "${out}/${file}.imports"
done
