#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Builds the deliberately broken Windows programs of the compatibility
# autopilot's CI from the sources beside this script, with mingw-w64. Every
# file is TUR's own; nothing that is built is committed, and no Microsoft
# file is used: mfc140u.def names a DLL, it does not contain one.
#
#   tests/windows-runtime/autopilot/build.sh OUTPUT_DIR
#
# Built, each in a directory of its own, as autopilot.py packs it:
#   winver/winver.exe          needs Windows 11 or later
#   dllover/dllover.exe        ships a d3dx9_43.dll that fails to initialise
#   dllover/d3dx9_43.dll
#   combo/combo.exe            both of the above
#   combo/d3dx9_43.dll
#   mfc/mfc.exe                imports mfc140u.dll, which nobody supplies
#   casepath/casepath.exe      reads Plugins\one.txt and Plugins\two.txt
#   codepage/codepage.exe      Japanese resources, crashes off code page 932
#   vdesktop/vdesktop.exe      refuses a screen larger than 1024x768
#   dpi/dpi.exe                refuses fewer than 120 DPI
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
out="${1:?output directory required}"
cc=x86_64-w64-mingw32-gcc
cflags=(-O1 -Wall -Wextra)

for name in winver dllover combo mfc casepath codepage vdesktop dpi; do
    mkdir -p "${out}/${name}"
done

"${cc}" "${cflags[@]}" -o "${out}/winver/winver.exe" "${here}/winver.c"

for name in dllover combo; do
    "${cc}" "${cflags[@]}" -shared -o "${out}/${name}/d3dx9_43.dll" "${here}/broken-d3dx9.c"
done
"${cc}" "${cflags[@]}" -o "${out}/dllover/dllover.exe" "${here}/dllover.c" "${out}/dllover/d3dx9_43.dll"
"${cc}" "${cflags[@]}" -DTUR_ALSO_VERSION -o "${out}/combo/combo.exe" "${here}/dllover.c" \
    "${out}/combo/d3dx9_43.dll"

x86_64-w64-mingw32-dlltool -d "${here}/mfc140u.def" -l "${out}/mfc/libmfc140u.a"
"${cc}" "${cflags[@]}" -o "${out}/mfc/mfc.exe" "${here}/mfc.c" "${out}/mfc/libmfc140u.a"
rm -f "${out}/mfc/libmfc140u.a"

"${cc}" "${cflags[@]}" -o "${out}/casepath/casepath.exe" "${here}/casepath.c"

x86_64-w64-mingw32-windres -O coff -o "${out}/codepage/codepage.res" "${here}/codepage.rc"
"${cc}" "${cflags[@]}" -o "${out}/codepage/codepage.exe" "${here}/codepage.c" \
    "${out}/codepage/codepage.res"
rm -f "${out}/codepage/codepage.res"

"${cc}" "${cflags[@]}" -o "${out}/vdesktop/vdesktop.exe" "${here}/vdesktop.c" -lgdi32 -luser32
"${cc}" "${cflags[@]}" -o "${out}/dpi/dpi.exe" "${here}/dpi.c" -lgdi32 -luser32

echo "Built:"
find "${out}" -type f -printf '  %P\n' | sort
for file in "${out}"/*/*; do
    printf '  %-28s %s\n' "${file#"${out}"/}" "$(file -b "${file}")"
done
echo "Imports:"
for file in "${out}"/*/*.exe; do
    printf '  %s: %s\n' "${file#"${out}"/}" \
        "$(x86_64-w64-mingw32-objdump -p "${file}" | awk '/DLL Name:/ {print $3}' | tr '\n' ' ')"
done
