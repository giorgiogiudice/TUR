#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The file systems of the layers experiment: ext4 as bootc makes TUR's root
# (no casefold feature), ext4 with the casefold feature, and btrfs, each in an
# image file on a loop device, each handed to the user the experiment runs as.
# Nothing on the machine's own file systems is changed.
#
#   fs-setup.sh DIRECTORY OWNER        (as root; OWNER as chown takes it)
#
# Used on the CI runner and inside the experiment's virtual machine only.
set -euo pipefail

dir="${1:?directory required}"
owner="${2:?owner required}"
mkdir -p "${dir}"

make() {
    local name="$1" size="$2"
    shift 2
    truncate -s "${size}" "${dir}/${name}.img"
    "$@" "${dir}/${name}.img"
    mkdir -p "${dir}/${name}"
    mount -o loop "${dir}/${name}.img" "${dir}/${name}"
    chown "${owner}" "${dir}/${name}"
}

make ext4 26G mkfs.ext4 -q -F -L tur-ext4
make ext4cf 12G mkfs.ext4 -q -F -O casefold -E encoding=utf8 -L tur-ext4cf
make btrfs 12G mkfs.btrfs -q -f -L tur-btrfs

for name in ext4 ext4cf btrfs; do
    findmnt -no SOURCE,FSTYPE,OPTIONS "${dir}/${name}"
done
for name in ext4 ext4cf; do
    echo "${name}: $(dumpe2fs -h "${dir}/${name}.img" 2>/dev/null | grep -Ei '^filesystem features|encoding' | tr -s ' \n' ' ')"
done
echo "mke2fs: $(mke2fs -V 2>&1 | head -n 1)"
echo "mkfs.btrfs: $(mkfs.btrfs --version 2>&1 | head -n 1)"
