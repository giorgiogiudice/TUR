#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Builds the windiff check programs with mingw-w64. The same binaries then
# run on Windows and under every Wine. Nothing built is committed.
#
#   tests/experiments/windiff/checks/build.sh OUTPUT_DIR
#
# A program that does not compile is recorded in OUTPUT_DIR/build.log and
# left out of OUTPUT_DIR/manifest.txt; the others are still built.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
out="${1:?output directory required}"
mkdir -p "${out}"
: >"${out}/build.log"
: >"${out}/manifest.txt"

cflags=(-O1 -g0 -Wall -Wextra -Wno-unused-function -Wno-unused-parameter -Wno-format -municode -fno-ident)
failed=0
for prog in fsbasic fsname proc con fuzz; do
    echo "== ${prog}" >>"${out}/build.log"
    if x86_64-w64-mingw32-gcc "${cflags[@]}" -o "${out}/${prog}.exe" "${here}/${prog}.c" -lshell32 >>"${out}/build.log" 2>&1; then
        echo "${prog}.exe" >>"${out}/manifest.txt"
    else
        echo "FAILED: ${prog}" | tee -a "${out}/build.log"
        failed=1
    fi
done
# Helpers the check programs start; not listed in the manifest.
for prog in wdchild wdlaunch; do
    echo "== ${prog}" >>"${out}/build.log"
    x86_64-w64-mingw32-gcc "${cflags[@]}" -o "${out}/${prog}.exe" "${here}/${prog}.c" -lshell32 >>"${out}/build.log" 2>&1 ||
        { echo "FAILED: ${prog}" | tee -a "${out}/build.log"; failed=1; }
done
x86_64-w64-mingw32-strip "${out}"/*.exe 2>/dev/null || true
x86_64-w64-mingw32-gcc --version | head -n 1 >"${out}/toolchain.txt"
rpm -q mingw64-gcc mingw64-headers mingw64-crt mingw64-binutils >>"${out}/toolchain.txt" 2>/dev/null || true
(cd "${out}" && sha256sum ./*.exe >sha256.txt)
echo "warnings: $(grep -c 'warning:' "${out}/build.log")"
grep -B2 -A6 -E 'error:|FAILED' "${out}/build.log" | head -n 150
cat "${out}/manifest.txt"
# The other jobs can run with whatever compiled, as long as the launcher did.
[ "${failed}" = 0 ] || echo "::warning::some windiff check programs did not compile; see build.log in the windiff-bin artifact"
[ -s "${out}/manifest.txt" ] && [ -f "${out}/wdlaunch.exe" ]
