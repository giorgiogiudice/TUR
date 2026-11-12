#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Build the boot-test image on top of a TUR image and install it to a raw disk
# image with the installer the image itself carries:
#
#     bootc install to-disk --generic-image --via-loopback
#
# as documented by bootc for the version in the image
# (https://github.com/bootc-dev/bootc, docs/src/bootc-installation.7.md).
#
#   tests/boot/build-disk.sh TUR_IMAGE WORK_DIR OUT_DIR
#
# WORK_DIR receives disk.raw. OUT_DIR/host receives the install log and what
# was decided. PODMAN names the container tool and must run as root, for
# example PODMAN="sudo podman": the installer needs a privileged container.
#
# Environment:
#   TUR_BOOT_DISK_SIZE   size of the disk image (default 20G, sparse)
#   TUR_BOOT_ROOT_FS     root filesystem to use when the image does not
#                        configure one (default ext4)
#
# Run this in CI only, never on a shared server.
set -euo pipefail

cd "$(dirname "$0")/../.."
image="${1:?TUR image reference required}"
work="${2:?work directory required}"
out="${3:?output directory required}"
read -r -a podman <<<"${PODMAN:-podman}"

test_image="${TUR_BOOT_TEST_IMAGE:-localhost/tur-boot-test:ci}"
disk_size="${TUR_BOOT_DISK_SIZE:-20G}"
fallback_fs="${TUR_BOOT_ROOT_FS:-ext4}"
# Kernel arguments added at install time, the documented way to make them
# machine-local state. The last console= becomes /dev/console, so the serial
# port carries the systemd status output and the agent's markers.
kargs=("console=tty0" "console=ttyS0,115200n8" "tur.boot_test=1")

mkdir -p "${work}" "${out}/host"
work="$(cd "${work}" && pwd)"
host="$(cd "${out}/host" && pwd)"

echo "== boot-test image: ${test_image} FROM ${image}"
# The context is the repository root: the layer also carries the fixtures of
# the P@x end-to-end test. What is sent is listed in the ignore file.
"${podman[@]}" build \
    --file tests/boot/guest/Containerfile \
    --ignorefile tests/boot/guest/Containerfile.containerignore \
    --build-arg "BASE=${image}" \
    --pull=never \
    --tag "${test_image}" \
    .
"${podman[@]}" image inspect -f 'image id: {{.Id}}  size: {{.Size}} bytes' "${test_image}"
"${podman[@]}" image inspect -f 'tur image id: {{.Id}}  size: {{.Size}} bytes' "${image}"

echo "== installer in the image"
"${podman[@]}" run --rm --network none "${test_image}" bootc --version | tee "${host}/bootc-version.txt"
"${podman[@]}" run --rm -i --network none "${test_image}" /bin/sh -s <<'INNER' | tee "${host}/install-config-files.txt"
for d in /usr/lib/bootc/install /etc/bootc/install /usr/lib/bootc/kargs.d /usr/lib/repart.d; do
    echo "--- $d"
    if [ -d "$d" ]; then
        for f in "$d"/*; do
            if [ -f "$f" ]; then echo "# $f"; cat "$f"; fi
        done
    else
        echo "(absent)"
    fi
done
INNER
"${podman[@]}" run --rm --network none "${test_image}" bootc install print-configuration --all \
    > "${host}/install-configuration.json" \
    || "${podman[@]}" run --rm --network none "${test_image}" bootc install print-configuration \
        > "${host}/install-configuration.json" \
    || echo "bootc install print-configuration failed"
echo "install configuration: $(cat "${host}/install-configuration.json")"

configured_fs="$(python3 - "${host}/install-configuration.json" <<'PY'
import json
import sys

try:
    config = json.load(open(sys.argv[1]))
except (OSError, ValueError):
    config = {}
root = ((config or {}).get("filesystem") or {}).get("root") or {}
print(root.get("type") or (config or {}).get("root-fs-type") or "")
PY
)"

install_args=(--generic-image --via-loopback)
if [ -n "${configured_fs}" ]; then
    root_fs="${configured_fs}"
    root_fs_source="image install configuration"
else
    root_fs="${fallback_fs}"
    root_fs_source="test default (the image configures none)"
    install_args+=(--filesystem "${root_fs}")
fi
for karg in "${kargs[@]}"; do
    install_args+=(--karg "${karg}")
done
echo "root filesystem: ${root_fs} (${root_fs_source})"

echo "== bootc install to-disk ${install_args[*]}"
rm -f "${work}/disk.raw"
truncate -s "${disk_size}" "${work}/disk.raw"
started="$(date +%s)"
"${podman[@]}" run --rm --privileged --pid=host --ipc=host \
    --security-opt label=type:unconfined_t \
    -v /dev:/dev \
    -v /var/lib/containers:/var/lib/containers \
    -v "${work}:/output" \
    "${test_image}" \
    bootc install to-disk "${install_args[@]}" /output/disk.raw 2>&1 | tee "${host}/install.log"
finished="$(date +%s)"

{
    echo "test_image=${test_image}"
    echo "test_image_id=$("${podman[@]}" image inspect -f '{{.Id}}' "${test_image}")"
    echo "tur_image=${image}"
    echo "tur_image_id=$("${podman[@]}" image inspect -f '{{.Id}}' "${image}")"
    echo "install_command=bootc install to-disk ${install_args[*]} /output/disk.raw"
    echo "install_seconds=$((finished - started))"
    echo "root_fs=${root_fs}"
    echo "root_fs_source=${root_fs_source}"
    echo "kargs=${kargs[*]}"
    echo "disk_size=${disk_size}"
    echo "disk_apparent_bytes=$(stat -c %s "${work}/disk.raw")"
    echo "disk_allocated_bytes=$(($(stat -c %b "${work}/disk.raw") * 512))"
} > "${host}/disk-info.env"
cat "${host}/disk-info.env"
ls -la "${work}"
