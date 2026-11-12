#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Builds the test programs of the windows-runtime end-to-end test from the C
# sources beside this script. Nothing that is built is committed.
#
#   tests/windows-runtime/build.sh OUTPUT_DIR
#
# Windows programs, with mingw-w64:
#   console64.exe   64-bit console program (console.c)
#   gui64.exe       64-bit GUI-subsystem program (gui.c)
#   console32.exe   32-bit console program, inspected only
#   library64.dll   64-bit DLL, inspected only
#   installer-fails.exe, installer-hangs.exe, installer-nothing.exe
#                   stand-ins for installers that go wrong (installer.c)
# Native files, with gcc, inspected only (native.c):
#   native-pie, native-nopie, native-static, native-static-pie,
#   libnative.so, native.o
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
out="${1:?output directory required}"
mkdir -p "${out}"

cflags=(-O1 -Wall -Wextra)

x86_64-w64-mingw32-gcc "${cflags[@]}" -o "${out}/console64.exe" "${here}/console.c"
x86_64-w64-mingw32-gcc "${cflags[@]}" -mwindows -o "${out}/gui64.exe" "${here}/gui.c"
i686-w64-mingw32-gcc "${cflags[@]}" -o "${out}/console32.exe" "${here}/console.c"
x86_64-w64-mingw32-gcc "${cflags[@]}" -shared -o "${out}/library64.dll" "${here}/native.c"

# Stand-ins for installers that go wrong (installer.c). Each is stripped, so
# that nothing follows its last section, and then given the NSIS first
# header as appended data: four bytes of flags, the signature 0xDEADBEEF and
# "NullsoftInst". That is what makes tur-runtime take it for an NSIS
# installer; it is not one.
for mode in fails hangs nothing; do
    x86_64-w64-mingw32-gcc "${cflags[@]}" -s "-DTUR_INSTALLER_${mode^^}" \
        -o "${out}/installer-${mode}.exe" "${here}/installer.c"
    printf '\000\000\000\000\357\276\255\336NullsoftInst' >>"${out}/installer-${mode}.exe"
done

gcc "${cflags[@]}" -o "${out}/native-pie" "${here}/native.c"
gcc "${cflags[@]}" -no-pie -o "${out}/native-nopie" "${here}/native.c"
gcc "${cflags[@]}" -static -o "${out}/native-static" "${here}/native.c"
gcc "${cflags[@]}" -static-pie -o "${out}/native-static-pie" "${here}/native.c"
gcc "${cflags[@]}" -shared -fPIC -o "${out}/libnative.so" "${here}/native.c"
gcc "${cflags[@]}" -c -o "${out}/native.o" "${here}/native.c"

echo "Built:"
for file in "${out}"/*; do
    printf '  %-22s %s\n' "$(basename "${file}")" "$(file -b "${file}")"
done
