#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Build the desktop builder image and save it for the Actions cache.
#
#   build-builder.sh DIR    writes DIR/image.tar.zst and DIR/packages.tsv
#
# Runs on the CI runner, never on a development machine (AGENTS.md).
set -eu

dir=${1:?usage: build-builder.sh DIR}
here=$(dirname "$0")
image=${BUILDER:-localhost/tur-desktop-builder:ci}

# shellcheck source=desktop/ci/builder.lock
. "$here/builder.lock"
base="${BASE_IMAGE}@${BASE_DIGEST}"
echo "base: $base (tag $BASE_TAG, verified $BASE_VERIFIED), builder revision $BUILDER_REVISION"

docker build --pull --build-arg "BASE=$base" -t "$image" -f "$here/builder/Containerfile" "$here/builder"

mkdir -p "$dir"
docker run --rm "$image" cat /usr/share/tur-builder/packages.tsv > "$dir/packages.tsv"
docker save "$image" | zstd -T0 -3 -q -o "$dir/image.tar.zst"

echo "image: $(docker image inspect -f '{{.Id}} {{.Size}}' "$image") bytes unpacked"
echo "saved: $(du -h "$dir/image.tar.zst" | cut -f1) compressed, $(wc -l < "$dir/packages.tsv") packages"
