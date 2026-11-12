#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Make a disposable disk image laid out like a Windows disk, without any
# Microsoft file: the installer-medium test's stand-in for the Windows disk
# beside which TUR is installed on a second disk.
#
#   tests/iso/make-windows-disk.sh DISK_IMAGE [SIZE]
#
# GPT, as Windows lays it out:
#   1  EFI system partition, FAT32, 260 MiB, holding EFI/Microsoft/Boot/ with
#      one placeholder text file named bootmgfw.efi (not a program)
#   2  Microsoft reserved partition, 16 MiB, empty
#   3  "Microsoft basic data", NTFS made by mkfs.ntfs (ntfs-3g), holding a
#      known file
#   4  "Microsoft basic data", 64 MiB, whose first sector carries the
#      BitLocker signature "-FVE-FS-" and nothing else: enough for the
#      installer's BitLocker check, not a BitLocker volume
#
# Prints the SHA-256 of the known file and of the whole image. Needs sudo for
# a loop device. Run in CI only.
set -euo pipefail

disk="${1:?disk image path required}"
size="${2:-20G}"
for tool in sfdisk mkfs.vfat mkfs.ntfs ntfscp ntfsls losetup; do
    command -v "${tool}" > /dev/null || { echo "missing tool: ${tool}" >&2; exit 1; }
done
files="$(mktemp -d)"
loop=""
cleanup() {
    if [ -n "${loop}" ]; then sudo losetup --detach "${loop}" || true; fi
    sudo umount "${files}/esp" 2> /dev/null || true
    rm -rf "${files}"
}
trap cleanup EXIT

printf 'TUR test placeholder. This is not a Microsoft file and not a program.\n' > "${files}/bootmgfw.efi"
printf 'This file stands for the data on a Windows disk that TUR must not touch.\n' > "${files}/keep-me.txt"
head -c 1048576 /dev/urandom >> "${files}/keep-me.txt"
known_sha256="$(sha256sum "${files}/keep-me.txt" | cut -d' ' -f1)"

rm -f "${disk}"
truncate -s "${size}" "${disk}"
sfdisk --quiet "${disk}" <<'TABLE'
label: gpt
start=2048, size=532480, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, name="EFI system partition"
size=32768, type=E3C9E316-0B5C-4DB8-817D-F92DF00215AE, name="Microsoft reserved partition"
size=10485760, type=EBD0A0A2-B9E5-4433-87C0-68B6B72699C7, name="Basic data partition"
size=131072, type=EBD0A0A2-B9E5-4433-87C0-68B6B72699C7, name="Basic data partition"
TABLE

loop="$(sudo losetup --find --show --partscan "${disk}")"
sudo udevadm settle --timeout=30 || true
sudo mkfs.vfat -F 32 -n SYSTEM "${loop}p1" > /dev/null
mkdir -p "${files}/esp"
sudo mount "${loop}p1" "${files}/esp"
sudo mkdir -p "${files}/esp/EFI/Microsoft/Boot"
sudo cp "${files}/bootmgfw.efi" "${files}/esp/EFI/Microsoft/Boot/bootmgfw.efi"
sudo umount "${files}/esp"
sudo mkfs.ntfs --quick --label Windows "${loop}p3" > /dev/null
sudo ntfscp "${loop}p3" "${files}/keep-me.txt" keep-me.txt
sudo ntfsls "${loop}p3"
printf '\353\130\220-FVE-FS-' | sudo dd of="${loop}p4" bs=512 count=1 conv=notrunc,sync status=none
sudo blkid "${loop}p1" "${loop}p2" "${loop}p3" "${loop}p4" || true
sudo losetup --detach "${loop}"
loop=""
sudo chown "$(id -u):$(id -g)" "${disk}"
sfdisk --list "${disk}"
echo "known_file_sha256=${known_sha256}"
echo "disk_sha256=$(sha256sum "${disk}" | cut -d' ' -f1)"
