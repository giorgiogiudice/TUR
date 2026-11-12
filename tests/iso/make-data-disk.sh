#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Make a disposable disk image that already holds data, for the installer
# medium's refusal test: a GPT with an ext4 filesystem that contains files and
# a second partition typed as Windows data.
#
#   tests/iso/make-data-disk.sh DISK_IMAGE [SIZE]
#
# Needs no root: the partition table and the filesystem are written straight
# into the image file. Run in CI only.
set -euo pipefail

disk="${1:?disk image path required}"
size="${2:-20G}"
data="$(mktemp -d)"
trap 'rm -rf "${data}"' EXIT

mkdir -p "${data}/documents"
printf 'This file stands for the data of someone who did not confirm.\n' > "${data}/documents/keep-me.txt"
head -c 4194304 /dev/urandom > "${data}/documents/photos.bin"
sha256sum "${data}/documents/"* | sed "s|${data}/||"

rm -f "${disk}"
truncate -s "${size}" "${disk}"
sfdisk --quiet "${disk}" <<'TABLE'
label: gpt
start=2048, size=2097152, type=0FC63DAF-8483-4772-8E79-3D69D8477DE4, name="old-linux"
type=EBD0A0A2-B9E5-4433-87C0-68B6B72699C7, name="old-windows"
TABLE
# The first partition starts at sector 2048 and is 1 GiB long.
mke2fs -q -F -t ext4 -L TURDATA -d "${data}" -E offset=1048576 "${disk}" 1000M
sfdisk --list "${disk}"
