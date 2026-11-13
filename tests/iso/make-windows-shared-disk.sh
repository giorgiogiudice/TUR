#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Make a disposable disk image laid out as Windows 10 or 11 leaves a disk
# after its own "Shrink Volume", without any Microsoft file: the test disk of
# "install TUR beside Windows on the same disk" (ADR-0028).
#
#   tests/iso/make-windows-shared-disk.sh DISK_IMAGE STANDIN_EFI [options]
#
#     --size-gib N      size of the disk (default 36)
#     --free-gib N      free space after the Windows partition (default 24; 0: none)
#     --esp-mib N       size of the EFI system partition (default 100: Windows'
#                       own size on a disk with 512-byte sectors)
#     --esp-fill-mib N  data in Windows' files on it (default 26, about what
#                       Windows 10 keeps there)
#     --bitlocker-c     give the Windows partition BitLocker's signature after
#                       its files are written, as an encrypted C: shows
#
# GPT, in Windows' order:
#   1  EFI system partition, FAT32, label SYSTEM:
#        EFI/Microsoft/Boot/bootmgfw.efi   the stand-in program STANDIN_EFI
#                                          (tests/iso/standin; not Microsoft's)
#        EFI/Boot/bootx64.efi              the same stand-in: Windows' fallback copy
#        EFI/Microsoft/Boot/BCD, bootmgr.efi, Fonts/*, en-US/*,
#        EFI/Microsoft/Recovery/BCD        random bytes, standing for Windows' files
#   2  Microsoft reserved partition, 16 MiB, empty
#   3  "Basic data partition", NTFS (mkfs.ntfs from ntfs-3g), label Windows,
#      with known files
#      (free space: what "Shrink Volume" left)
#   4  "Windows recovery environment" at the end of the disk, 600 MiB, NTFS,
#      label Recovery, GPT attributes required + no drive letter
#
# Prints the layout. Needs sudo for a loop device. Run in CI only.
set -euo pipefail

disk="${1:?disk image path required}"
standin="${2:?the stand-in EFI program is required}"
shift 2
size_gib=36 free_gib=24 esp_mib=100 esp_fill_mib=26 bitlocker_c=no
while [ "$#" -gt 0 ]; do
    case "$1" in
        --size-gib) size_gib="$2"; shift 2 ;;
        --free-gib) free_gib="$2"; shift 2 ;;
        --esp-mib) esp_mib="$2"; shift 2 ;;
        --esp-fill-mib) esp_fill_mib="$2"; shift 2 ;;
        --bitlocker-c) bitlocker_c=yes; shift ;;
        *) echo "unknown option $1" >&2; exit 2 ;;
    esac
done
for tool in sfdisk mkfs.vfat mkfs.ntfs ntfscp ntfsls losetup; do
    command -v "${tool}" > /dev/null || { echo "missing tool: ${tool}" >&2; exit 1; }
done
files="$(mktemp -d)"
loop=""
cleanup() {
    sudo umount "${files}/esp" 2> /dev/null || true
    if [ -n "${loop}" ]; then sudo losetup --detach "${loop}" || true; fi
    rm -rf "${files}"
}
trap cleanup EXIT

mib=$((1024 * 1024 / 512))
total=$((size_gib * 1024 * mib))
esp_start=$mib esp_size=$((esp_mib * mib))
msr_start=$((esp_start + esp_size)) msr_size=$((16 * mib))
c_start=$((msr_start + msr_size))
recovery_size=$((600 * mib))
recovery_start=$((total - mib - recovery_size))
c_size=$((recovery_start - c_start - free_gib * 1024 * mib))
[ "${c_size}" -gt $((1024 * mib)) ] || { echo "the disk is too small for this layout" >&2; exit 2; }

rm -f "${disk}"
truncate -s "${size_gib}G" "${disk}"
sfdisk --quiet "${disk}" << TABLE
label: gpt
start=${esp_start}, size=${esp_size}, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, name="EFI system partition"
start=${msr_start}, size=${msr_size}, type=E3C9E316-0B5C-4DB8-817D-F92DF00215AE, name="Microsoft reserved partition"
start=${c_start}, size=${c_size}, type=EBD0A0A2-B9E5-4433-87C0-68B6B72699C7, name="Basic data partition"
start=${recovery_start}, size=${recovery_size}, type=DE94BBA4-06D1-4D40-A16A-BFD50179D6AC, name="Basic data partition", attrs="RequiredPartition GUID:63"
TABLE

# Windows' files on the EFI system partition: random bytes under Windows'
# names, ESP_FILL_MIB in all, and the stand-in where the boot manager is.
mkdir -p "${files}/tree/EFI/Microsoft/Boot/Fonts" "${files}/tree/EFI/Microsoft/Boot/en-US" \
    "${files}/tree/EFI/Microsoft/Recovery" "${files}/tree/EFI/Boot"
cp "${standin}" "${files}/tree/EFI/Microsoft/Boot/bootmgfw.efi"
cp "${standin}" "${files}/tree/EFI/Boot/bootx64.efi"
head -c 32768 /dev/urandom > "${files}/tree/EFI/Microsoft/Boot/BCD"
head -c 28672 /dev/urandom > "${files}/tree/EFI/Microsoft/Recovery/BCD"
head -c 1290240 /dev/urandom > "${files}/tree/EFI/Microsoft/Boot/bootmgr.efi"
head -c 1495040 /dev/urandom > "${files}/tree/EFI/Microsoft/Boot/memtest.efi"
head -c 81920 /dev/urandom > "${files}/tree/EFI/Microsoft/Boot/en-US/bootmgfw.efi.mui"
remaining=$((esp_fill_mib * 1024 * 1024 - 32768 - 28672 - 1290240 - 1495040 - 81920))
number=0
while [ "${remaining}" -gt 0 ]; do
    chunk=$((remaining > 4194304 ? 4194304 : remaining))
    head -c "${chunk}" /dev/urandom > "${files}/tree/EFI/Microsoft/Boot/Fonts/font${number}.ttf"
    remaining=$((remaining - chunk))
    number=$((number + 1))
done

printf 'This file stands for the data on a Windows disk that TUR must not touch.\n' > "${files}/keep-me.txt"
head -c 1048576 /dev/urandom >> "${files}/keep-me.txt"
head -c 4194304 /dev/urandom > "${files}/pagefile-standin.bin"
printf 'Recovery partition stand-in.\n' > "${files}/recovery.txt"

loop="$(sudo losetup --find --show --partscan "${disk}")"
sudo udevadm settle --timeout=30 || true
sudo mkfs.vfat -F 32 -n SYSTEM "${loop}p1" > /dev/null
mkdir -p "${files}/esp"
sudo mount "${loop}p1" "${files}/esp"
sudo cp -r "${files}/tree/EFI" "${files}/esp/"
sync
df -B1 "${files}/esp"
sudo umount "${files}/esp"
sudo mkfs.ntfs --quick --label Windows "${loop}p3" > /dev/null
sudo ntfscp "${loop}p3" "${files}/keep-me.txt" keep-me.txt
sudo ntfscp "${loop}p3" "${files}/pagefile-standin.bin" pagefile-standin.bin
sudo ntfsls "${loop}p3"
sudo mkfs.ntfs --quick --label Recovery "${loop}p4" > /dev/null
sudo ntfscp "${loop}p4" "${files}/recovery.txt" recovery.txt
if [ "${bitlocker_c}" = yes ]; then
    printf '\353\130\220-FVE-FS-' | sudo dd of="${loop}p3" bs=512 count=1 conv=notrunc,sync status=none
fi
sudo blkid "${loop}p1" "${loop}p2" "${loop}p3" "${loop}p4" || true
sudo losetup --detach "${loop}"
loop=""
sudo chown "$(id -u):$(id -g)" "${disk}"
sfdisk --list "${disk}"
sfdisk --list-free "${disk}" || true
echo "layout: size_gib=${size_gib} free_gib=${free_gib} esp_mib=${esp_mib} esp_fill_mib=${esp_fill_mib} bitlocker_c=${bitlocker_c}"
