#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Builds the two test programs of the runtime-layer test with mingw-w64.
#
#   tests/experiments/layers/rt/build.sh OUTPUT_DIR
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
out="${1:?output directory required}"
cc=x86_64-w64-mingw32-gcc
mkdir -p "${out}"
"${cc}" -O1 -Wall -Wextra -shared -o "${out}/turrt.dll" "${here}/turrt.c" \
    -Wl,--out-implib,"${out}/libturrt.a"
"${cc}" -O1 -Wall -Wextra -o "${out}/needs.exe" "${here}/needs.c" "${out}/libturrt.a" -ladvapi32
rm -f "${out}/libturrt.a"
ls -l "${out}"
