#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Build tests/iso/standin/standin.c, the stand-in for Windows Boot Manager
# that the "beside Windows on the same disk" test disk carries (it is not a
# Microsoft program), as an x86-64 EFI application with gnu-efi.
#
#   tests/iso/build-standin.sh OUTPUT.efi
#
# Needs the Ubuntu packages gnu-efi and binutils. Run in CI only (ADR-0002).
set -euo pipefail

out="${1:?output path required}"
src="$(cd "$(dirname "$0")" && pwd)/standin/standin.c"
work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT

# Where this release of gnu-efi keeps its start code, linker script and
# libraries: /usr/lib on current Ubuntu, a lib64 or arch directory elsewhere.
find_one() {
    local name="$1" found
    found="$(dpkg -L gnu-efi 2> /dev/null | grep -E "/${name}\$" | head -n 1)"
    if [ -z "${found}" ]; then
        found="$(find /usr/lib /usr/lib64 /usr/lib/x86_64-linux-gnu -maxdepth 3 -name "${name}" 2> /dev/null | head -n 1)"
    fi
    [ -n "${found}" ] || { echo "gnu-efi: ${name} not found" >&2; exit 1; }
    echo "${found}"
}
crt0="$(find_one crt0-efi-x86_64.o)"
lds="$(find_one elf_x86_64_efi.lds)"
libdir="$(dirname "$(find_one libgnuefi.a)")"
include=/usr/include/efi
echo "gnu-efi: $(dpkg-query -W -f='${Version}' gnu-efi 2> /dev/null || echo unknown); crt0 ${crt0}; lds ${lds}; libs ${libdir}"

gcc -I"${include}" -I"${include}/x86_64" -I"${include}/protocol" \
    -fpic -ffreestanding -fno-stack-protector -fno-stack-check -fshort-wchar -mno-red-zone \
    -maccumulate-outgoing-args -DEFI_FUNCTION_WRAPPER -Wall -O2 \
    -c "${src}" -o "${work}/standin.o"
ld -shared -Bsymbolic -nostdlib -znocombreloc -T "${lds}" "${crt0}" "${work}/standin.o" \
    -L"${libdir}" -lefi -lgnuefi -o "${work}/standin.so"
objcopy -j .text -j .sdata -j .data -j .dynamic -j .dynsym -j .rel -j .rela -j '.rel.*' -j '.rela.*' \
    -j .reloc -j .rodata --target efi-app-x86_64 --subsystem=10 "${work}/standin.so" "${out}"
file "${out}" || true
ls -la "${out}"
sha256sum "${out}"
