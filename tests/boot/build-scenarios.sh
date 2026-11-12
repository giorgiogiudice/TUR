#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Build the boot-health scenario images and put them on a disk the guest can
# update from without a registry or a network (design sections 11.2, 11.3).
#
#   tests/boot/build-scenarios.sh TEST_IMAGE WORK_DIR OUT_DIR NAME [NAME ...]
#
# Each NAME is a directory under tests/boot/scenarios with a Containerfile that
# is built FROM the boot-test image with one deliberate change. All of them are
# copied into one OCI layout, one reference per scenario, with their layers
# left uncompressed: the layers then have the digests the installed system
# already knows, so the guest fetches only what changed. The layout is written
# into an ext4 image, WORK_DIR/images.img, which the scenario driver attaches
# to the guest as a second, read-only disk. In the guest:
#
#     bootc switch --transport oci /run/tur-test/images:NAME
#
# PODMAN and SKOPEO name the tools and must use the same storage, for example
# PODMAN="sudo podman" SKOPEO="sudo skopeo".
#
# Run this in CI only, never on a shared server.
set -euo pipefail

cd "$(dirname "$0")/../.."
test_image="${1:?boot-test image reference required}"
work="${2:?work directory required}"
out="${3:?output directory required}"
shift 3
if [ "$#" -eq 0 ]; then
    echo "at least one scenario image name is required" >&2
    exit 2
fi
read -r -a podman <<<"${PODMAN:-podman}"
read -r -a skopeo <<<"${SKOPEO:-skopeo}"

mkdir -p "${work}" "${out}/host"
work="$(cd "${work}" && pwd)"
host="$(cd "${out}/host" && pwd)"
layout="${work}/images"
sudo rm -rf "${layout}"
mkdir -p "${layout}"
: > "${host}/scenario-images.txt"

"${skopeo[@]}" --version
"${skopeo[@]}" inspect --raw "containers-storage:${test_image}" > "${host}/test-image-manifest.json"

started="$(date +%s)"
for name in "$@"; do
    tag="localhost/tur-scenario-${name}:ci"
    echo "== scenario image ${name}: ${tag} FROM ${test_image}"
    "${podman[@]}" build \
        --file "tests/boot/scenarios/${name}/Containerfile" \
        --build-arg "BASE=${test_image}" \
        --pull=never \
        --tag "${tag}" \
        "tests/boot/scenarios/${name}"
    copy_started="$(date +%s)"
    "${skopeo[@]}" copy --quiet --dest-oci-accept-uncompressed-layers \
        "containers-storage:${tag}" "oci:${layout}:${name}"
    "${skopeo[@]}" inspect --raw "oci:${layout}:${name}" > "${host}/scenario-${name}-manifest.json"
    {
        echo "${name}"
        echo "  image id:   $("${podman[@]}" image inspect -f '{{.Id}}' "${tag}")"
        echo "  copy:       $(($(date +%s) - copy_started)) s"
        python3 - "${host}/test-image-manifest.json" "${host}/scenario-${name}-manifest.json" <<'PY'
import json
import sys

base = json.load(open(sys.argv[1]))
this = json.load(open(sys.argv[2]))
known = {layer["digest"] for layer in base.get("layers", [])}
layers = this.get("layers", [])
shared = sum(1 for layer in layers if layer["digest"] in known)
kinds = sorted({layer.get("mediaType", "?").rsplit(".", 1)[-1] for layer in layers})
print(f"  layers:     {len(layers)}, of which {shared} have a digest the boot-test image also has; media types end in {kinds}")
print(f"  new layers: {[layer['digest'][:19] + ' ' + str(layer.get('size')) + ' bytes' for layer in layers if layer['digest'] not in known]}")
PY
    } | tee -a "${host}/scenario-images.txt"
done

bytes="$(sudo du -sb "${layout}" | cut -f1)"
megabytes="$((bytes * 11 / 10 / 1048576 + 512))"
echo "== OCI layout: ${bytes} bytes in ${layout}; image disk: ${megabytes} MiB"
sudo cat "${layout}/index.json" | tee "${host}/scenario-index.json" > /dev/null
rm -f "${work}/images.img"
truncate -s "${megabytes}M" "${work}/images.img"
sudo mke2fs -q -F -t ext4 -L tur-test-images -m 0 -d "${layout}" "${work}/images.img"
sudo chown "$(id -u):$(id -g)" "${work}/images.img"
sudo rm -rf "${layout}"
{
    echo "scenario_images=$*"
    echo "oci_layout_bytes=${bytes}"
    echo "image_disk_megabytes=${megabytes}"
    echo "seconds=$(($(date +%s) - started))"
} | tee "${host}/scenario-images.env"
ls -la "${work}"
