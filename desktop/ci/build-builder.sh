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

# TUR_BUILD_CONTEXTS, from .github/actions/use-base-mirror: --build-context
# flags that take the base from TUR's mirror, which keeps the digest after
# quay.io has dropped it. The FROM line still names the base by its quay.io
# reference; only where the bytes come from changes.
echo "contexts: ${TUR_BUILD_CONTEXTS:-none (the base comes from $BASE_IMAGE)}"
# shellcheck disable=SC2086 # TUR_BUILD_CONTEXTS is a list of flags, one word each
docker build --pull ${TUR_BUILD_CONTEXTS:-} --build-arg "BASE=$base" -t "$image" -f "$here/builder/Containerfile" "$here/builder"

mkdir -p "$dir"
docker run --rm "$image" cat /usr/share/tur-builder/packages.tsv > "$dir/packages.tsv"
docker save "$image" | zstd -T0 -3 -q -o "$dir/image.tar.zst"

echo "image: $(docker image inspect -f '{{.Id}} {{.Size}}' "$image") bytes unpacked"
echo "saved: $(du -h "$dir/image.tar.zst" | cut -f1) compressed, $(wc -l < "$dir/packages.tsv") packages"
