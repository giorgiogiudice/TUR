#!/bin/sh
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Load the builder image that the `builder` job put in the Actions cache.
#
#   load-builder.sh DIR     reads DIR/image.tar.zst
#
# Runs on the CI runner, never on a development machine (AGENTS.md).
set -eu

dir=${1:?usage: load-builder.sh DIR}
image=${BUILDER:-localhost/tur-desktop-builder:ci}

start=$(date +%s)
zstd -d -c "$dir/image.tar.zst" | docker load
echo "loaded $(du -h "$dir/image.tar.zst" | cut -f1) in $(($(date +%s) - start)) s"
docker image inspect -f 'image: {{.Id}} created {{.Created}}' "$image"
